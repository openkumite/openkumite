//! The game data, converted in the game from the player's own copies (natively): the Tekken 3 pack is the assets'
//! folder itself (`ROOT`, as kumite_converter wrote it before: OpenKumite's places find it by its manifest.json),
//! and beside its files
//!
//! ```text
//! ROOT/packs/t2/       the Tekken 2 pack (ok_tekken2_converter: its disc alone)
//! ROOT/packs/cs/       the Counter-Strike pack (ok_hl_cs_converter: the game folder alone)
//! ROOT/extras/         the packs installed: the files they add to or change in the Tekken 3 pack, and manifest.json
//! ```
//!
//! The game reads the layers ([`activate`]: kumite_fs): a file of ROOT from `extras/`, else a pack's `assets/` (the
//! Tekken 2 pack's music and endings), else ROOT. So the extras' manifest.json is the one the game reads: the Tekken
//! 3 pack's, with the packs and the fingerprint of the whole (online play compares it), and what it was installed
//! from (`installed`: the Tekken 3 pack's fingerprint and the packs' sources); when that changes, the packs are
//! installed again. The fingerprint covers characters/, stages/ and gamedata/ only, so packs/ and extras/ are no
//! part of the Tekken 3 pack's own.

use anyhow::{bail, Context, Result};
use ok_converter_common::io::{Ctx, DiscFile, Layered, StdDisc, StdFs, Vfs};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub const PACKS: &str = "packs";
pub const EXTRAS: &str = "extras";

/// The game data's parts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    /// Tekken 3: the game (required), ROOT itself.
    T3,
    /// Tekken 2: its fighters (optional), a pack.
    T2,
    /// Counter-Strike: its levels as arenas (optional), a pack.
    Cs,
}

impl Part {
    pub const ALL: [Part; 3] = [Part::T3, Part::T2, Part::Cs];
    /// The packs, in their install order (kumite_converter's: Counter-Strike's then Tekken 2's).
    const PACKS: [Part; 2] = [Part::Cs, Part::T2];

    pub fn name(self) -> &'static str {
        match self {
            Part::T3 => "Tekken 3",
            Part::T2 => "Tekken 2",
            Part::Cs => "Counter-Strike",
        }
    }

    /// The pack's folder (Tekken 3: ROOT).
    pub fn dir(self, root: &Path) -> PathBuf {
        match self {
            Part::T3 => root.to_path_buf(),
            Part::T2 => root.join(PACKS).join(ok_tekken2_converter::pack::KIND),
            Part::Cs => root.join(PACKS).join(ok_hl_cs_converter::pack::KIND),
        }
    }

    /// The pack's manifest, when it holds one of the format this game reads.
    pub fn manifest(self, root: &Path) -> Option<Value> {
        let dir = StdFs(self.dir(root));
        match self {
            Part::T3 => {
                kumite_assets::Assets::new(root).ok()?;
                serde_json::from_slice(&std::fs::read(root.join("manifest.json")).ok()?).ok()
            }
            Part::T2 => ok_tekken2_converter::pack::manifest(&dir).ok(),
            Part::Cs => ok_hl_cs_converter::pack::manifest(&dir).ok(),
        }
    }
}

/// What the game data's parts are now.
#[derive(Clone, Debug, Default)]
pub struct State {
    /// Each part's manifest (None: not converted, or another format: converted again).
    pub t3: Option<Value>,
    pub t2: Option<Value>,
    pub cs: Option<Value>,
    /// The packs there are installed, as they are (the extras are theirs).
    pub installed: bool,
}

impl State {
    pub fn of(root: &Path) -> Self {
        let mut s = Self { t3: Part::T3.manifest(root), t2: Part::T2.manifest(root), cs: Part::Cs.manifest(root), installed: false };
        s.installed = !s.has_packs() || read_json(&root.join(EXTRAS).join("manifest.json")).is_some_and(|m| s.t3.is_some() && m["installed"] == s.stamp());
        s
    }

    pub fn get(&self, part: Part) -> Option<&Value> {
        match part {
            Part::T3 => self.t3.as_ref(),
            Part::T2 => self.t2.as_ref(),
            Part::Cs => self.cs.as_ref(),
        }
    }

    pub fn has_packs(&self) -> bool {
        self.t2.is_some() || self.cs.is_some()
    }

    /// The game can start: Tekken 3 converted, the packs installed.
    pub fn playable(&self) -> bool {
        self.t3.is_some() && self.installed
    }

    /// What the extras are made of: when it changes they are installed again.
    fn stamp(&self) -> Value {
        json!({
            "t3": self.t3.as_ref().map(|m| m["fingerprint"].clone()),
            "t2": self.t2.as_ref().map(|m| json!({"format": m["format"], "source": m["source"]})),
            "cs": self.cs.as_ref().map(|m| json!({"format": m["format"], "levels": m["levels"]})),
        })
    }
}

fn read_json(path: &Path) -> Option<Value> {
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

/// The layers the game reads over ROOT, the first first: the extras, then the packs' own files.
fn layers(root: &Path, state: &State) -> Vec<PathBuf> {
    if !state.has_packs() {
        return Vec::new();
    }
    let mut out = vec![root.join(EXTRAS)];
    for part in Part::PACKS.into_iter().rev() {
        let assets = part.dir(root).join(ok_tekken2_converter::pack::ASSETS);
        if state.get(part).is_some() && assets.is_dir() {
            out.push(assets);
        }
    }
    out
}

/// The game reads ROOT's layers from now on (none while the packs are not installed: Tekken 3 alone).
pub fn activate(root: &Path) -> State {
    let state = State::of(root);
    kumite_fs::set_layers(root, if state.installed { layers(root, &state) } else { Vec::new() });
    state
}

/// A job of the Game data screen.
#[derive(Clone, Debug)]
pub enum Job {
    /// Convert a part from the player's copy: a disc image's track 1 (Tekken 3, Tekken 2) or the Half-Life folder
    /// (Counter-Strike); then the packs installed again.
    Convert(Part, PathBuf),
    /// Remove a pack (or the Tekken 3 pack's files); the rest installed again.
    Remove(Part),
    /// Install the packs (into the extras) as they are.
    Install,
}

/// Runs `job` on the game data in `root`, its progress in `log` (one line at a time).
pub fn run(root: &Path, job: &Job, log: &(dyn Fn(&str) + Sync)) -> Result<()> {
    std::fs::create_dir_all(root).with_context(|| format!("cannot make {}", root.display()))?;
    let jobs = std::thread::available_parallelism().map_or(1, |n| n.get());
    match job {
        Job::Convert(part, from) => {
            let out = StdFs(part.dir(root));
            let ctx = Ctx { out: &out, log, jobs };
            match part {
                Part::T3 => {
                    let disc = disc(from)?;
                    let track2 = beside(from, 2);
                    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
                    ok_tekken3_converter::convert_all::convert_resumable(&ctx, &disc, track2.as_ref().map(|t| t as &dyn DiscFile), &now)?;
                }
                Part::T2 => {
                    let disc = disc(from)?;
                    let cdda = [2, 3].map(|n| beside(from, n).map(|t| Box::new(t) as Box<dyn DiscFile>));
                    ok_tekken2_converter::pack::convert(&ctx, &disc, &cdda)?;
                }
                Part::Cs => {
                    let (game, _) = ok_hl_cs_converter::game::game_root(from)?;
                    ok_hl_cs_converter::pack::convert(&ctx, &StdFs(game))?;
                }
            }
        }
        Job::Remove(Part::T3) => {
            // (the Tekken 3 pack's files; packs/ stays, the extras go: they need it)
            for dir in ["characters", "stages", "gamedata", "pictures", "audio", "video", EXTRAS, "manifest.json"] {
                remove(&root.join(dir))?;
            }
        }
        Job::Remove(part) => remove(&part.dir(root))?,
        Job::Install => {}
    }
    install(root, log)
}

/// Installs the packs there are into the extras (made anew), with its manifest; nothing when there is no Tekken 3
/// pack or no other.
pub fn install(root: &Path, log: &(dyn Fn(&str) + Sync)) -> Result<()> {
    let extras = root.join(EXTRAS);
    remove(&extras)?;
    let state = State::of(root);
    if state.t3.is_none() || !state.has_packs() {
        return Ok(());
    }
    let (top, base) = (StdFs(extras.clone()), StdFs(root.to_path_buf()));
    let below: Vec<StdFs> = layers(root, &state).into_iter().skip(1).map(StdFs).collect();
    let mut view = Layered { top: &top, below: below.iter().map(|l| l as &dyn Vfs).collect() };
    view.below.push(&base);
    let ctx = Ctx { out: &view, log, jobs: 1 };
    let mut packs = serde_json::Map::new();
    for part in Part::PACKS {
        let Some(m) = state.get(part) else { continue };
        let pack = StdFs(part.dir(root));
        log(&format!("[install] {}", part.name()));
        match part {
            Part::Cs => {
                ok_hl_cs_converter::pack::install(&ctx, &pack).context("Counter-Strike")?;
                packs.insert("cs".into(), m["levels"].clone());
            }
            Part::T2 => {
                ok_tekken2_converter::pack::install(&ctx, &pack).context("Tekken 2")?;
                packs.insert("t2".into(), m["source"].clone());
            }
            Part::T3 => {}
        }
    }
    // the Tekken 3 manifest, with the packs (as kumite_converter wrote it with extras) and what they were installed
    // from; its fingerprint the whole's
    let mut manifest = state.t3.clone().and_then(|m| m.as_object().cloned()).unwrap_or_default();
    manifest.insert("fingerprint".into(), json!(""));
    manifest.insert("packs".into(), Value::Object(packs));
    manifest.insert("installed".into(), state.stamp());
    log("[install] the fingerprint");
    manifest.insert("fingerprint".into(), json!(ok_tekken3_converter::convert_all::fingerprint(&view)?));
    top.write("manifest.json", serde_json::to_string_pretty(&Value::Object(manifest))?.as_bytes())?;
    Ok(())
}

fn disc(path: &Path) -> Result<StdDisc> {
    if !path.is_file() {
        bail!("there is no disc image at {}", path.display());
    }
    Ok(StdDisc(path.to_path_buf()))
}

/// The disc's track `n` beside its track 1 (`... (Track 1).bin`), when it is there.
fn beside(track1: &Path, n: usize) -> Option<StdDisc> {
    let name = track1.file_name()?.to_string_lossy();
    let other = name.replace("Track 1", &format!("Track {n}"));
    let path = track1.with_file_name(other);
    (path != track1 && path.is_file()).then_some(StdDisc(path))
}

fn remove(path: &Path) -> Result<()> {
    let r = if path.is_dir() { std::fs::remove_dir_all(path) } else { std::fs::remove_file(path) };
    match r {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e).with_context(|| format!("cannot remove {}", path.display())),
        _ => Ok(()),
    }
}

/// The bytes of the files under `dir`.
pub fn size_of(dir: &Path) -> u64 {
    std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| if e.path().is_dir() { size_of(&e.path()) } else { e.metadata().map_or(0, |m| m.len()) }).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tracks_beside_track_1() {
        let dir = std::env::temp_dir().join(format!("ok_packs_beside_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for t in [1, 2] {
            std::fs::write(dir.join(format!("Game (Track {t}).bin")), b"").unwrap();
        }
        let t1 = dir.join("Game (Track 1).bin");
        assert_eq!(beside(&t1, 2).map(|d| d.0), Some(dir.join("Game (Track 2).bin")));
        assert!(beside(&t1, 3).is_none());
        assert!(beside(&dir.join("game.bin"), 2).is_none());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn no_packs_no_layers_and_nothing_to_install() {
        let dir = std::env::temp_dir().join(format!("ok_packs_none_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let s = State::of(&dir);
        assert!(s.t3.is_none() && !s.has_packs() && s.installed && !s.playable());
        assert!(layers(&dir, &s).is_empty());
        install(&dir, &|_| {}).unwrap();
        assert!(!dir.join(EXTRAS).exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// The Game data screen's jobs from the player's copies to the game's view:
    /// `OK_T3_BIN=... OK_T2_BIN=... OK_CS_DIR=... OK_PACKS_ROOT=<an empty folder> OK_PACKS_FINGERPRINT=<kumite_convert's>
    /// cargo test --release -p openkumite -- --ignored the_packs_converted_in_the_game`.
    #[test]
    #[ignore]
    fn the_packs_converted_in_the_game() {
        let var = |k: &str| std::env::var(k).unwrap_or_else(|_| panic!("set {k}"));
        let root = PathBuf::from(var("OK_PACKS_ROOT"));
        let log = |l: &str| println!("{l}");
        for (part, from) in [(Part::T3, "OK_T3_BIN"), (Part::T2, "OK_T2_BIN"), (Part::Cs, "OK_CS_DIR")] {
            run(&root, &Job::Convert(part, var(from).into()), &log).unwrap();
        }
        let state = activate(&root);
        assert!(state.playable() && state.t2.is_some() && state.cs.is_some());
        let assets = kumite_assets::Assets::new(&root).unwrap();
        assert_eq!(assets.fingerprint(), var("OK_PACKS_FINGERPRINT"));
        // the layers: a Tekken 2 fighter's model from the extras, its music from the pack, Tekken 3's own from ROOT
        assert!(kumite_fs::resolve(&root.join("characters/model_900.glb")).starts_with(root.join(EXTRAS)));
        let music = kumite_fs::resolve(&root.join("audio/music/t2_00.flac"));
        assert!(music.starts_with(Part::T2.dir(&root)), "{}", music.display());
        assert_eq!(kumite_fs::resolve(&root.join("characters/model_000.glb")), root.join("characters/model_000.glb"));
        // removed again: Tekken 3 alone, no layers
        run(&root, &Job::Remove(Part::T2), &log).unwrap();
        run(&root, &Job::Remove(Part::Cs), &log).unwrap();
        let state = activate(&root);
        assert!(state.playable() && !state.has_packs() && !root.join(EXTRAS).exists());
        assert_eq!(kumite_assets::Assets::new(&root).unwrap().fingerprint(), "faec270495bc58ba434d29b74cabef14");
    }
}
