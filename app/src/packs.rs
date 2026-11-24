//! The game data, converted in the game from the player's own copies (natively): the Tekken 3 pack is the assets'
//! folder itself (`ROOT`, as kumite_converter wrote it before: OpenKumite's places find it by its manifest.json),
//! and beside its files
//!
//! ```text
//! ROOT/packs/t2/       the Tekken 2 pack (ok_tekken2_converter: its disc alone)
//! ROOT/packs/cs/       the Counter-Strike pack (ok_hl_cs_converter: the game folder alone)
//! ROOT/packs/t3arcade/ the Tekken 3 arcade pack (ok_tekken3_arcade_converter: the ROM set alone); not installed, its
//!                      `assets/arcade/` stages are read as they are
//! ROOT/extras/         the packs installed: the files they add to or change in the Tekken 3 pack, and manifest.json
//! ROOT/packs/ttt/      the Tekken Tag Tournament pack (ok_tekken_tag_converter: its disc alone); not installed, a
//!                      game of its own: its `assets/` the game root when it is the main game ([`game`])
//! ROOT/packs/ttt/extras/ what the Tekken 3 pack brings to Tekken Tag Tournament ([`install_ttt`]), and manifest.json
//! ```
//!
//! The game reads the layers ([`activate`]: kumite_fs): a file of ROOT from `extras/`, else a pack's `assets/` (the
//! Tekken 2 pack's music and endings), else ROOT. So the extras' manifest.json is the one the game reads: the Tekken
//! 3 pack's, with the packs and the fingerprint of the whole (online play compares it), and what it was installed
//! from (`installed`: the Tekken 3 pack's fingerprint and the packs' sources); when that changes, the packs are
//! installed again. The fingerprint covers characters/, stages/ and gamedata/ only, so packs/ and extras/ are no
//! part of the Tekken 3 pack's own. With Tekken Tag Tournament the main game, its game root's layers are its extras,
//! then the arcade pack's `assets/` ([`ttt_layers`]).

use anyhow::{bail, Context, Result};
pub use kumite_packs::{Game, Part};
use ok_converter_common::disc::Disc;
use ok_converter_common::io::{Ctx, DiscFile, StdFs, Vfs};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

pub const PACKS: &str = "packs";
pub const EXTRAS: &str = "extras";

/// The part's folder (Tekken 3: ROOT).
pub fn dir(part: Part, root: &Path) -> PathBuf {
    match part {
        Part::T3 => root.to_path_buf(),
        p => root.join(PACKS).join(p.kind()),
    }
}

/// The main game's root: Tekken 3's is ROOT, Tekken Tag Tournament's its pack's `assets/`.
pub fn game_root(root: &Path, game: Game) -> PathBuf {
    match game {
        Game::T3 => root.to_path_buf(),
        Game::Ttt => dir(Part::Ttt, root).join(kumite_packs::ASSETS),
    }
}

/// The main game, as the player chose it (the save's `openkumite_game`; none: Tekken 3).
pub fn game() -> Game {
    if crate::save::main_game().as_deref() == Some(Part::Ttt.kind()) {
        Game::Ttt
    } else {
        Game::T3
    }
}

/// What the game data's parts are now.
#[derive(Clone, Debug, Default)]
pub struct State {
    /// Each part's manifest (None: not converted, or another format: converted again).
    pub manifests: kumite_packs::Manifests,
    /// The packs there are installed, as they are (the extras are theirs).
    pub installed: bool,
    /// The main game.
    pub game: Game,
    /// Tekken Tag Tournament's pack has its game root (assets/; a pack of the converter's research layout has none).
    pub ttt_assets: bool,
}

impl State {
    pub fn of(root: &Path, game: Game) -> Self {
        let m = |p: Part| p.manifest(&StdFs(dir(p, root)));
        let manifests = kumite_packs::Manifests { t3: m(Part::T3), t2: m(Part::T2), cs: m(Part::Cs), arcade: m(Part::Arcade), ttt: m(Part::Ttt) };
        let installed = manifests.installed(&StdFs(root.join(EXTRAS)));
        Self { manifests, installed, game, ttt_assets: game_root(root, Game::Ttt).is_dir() }
    }

    pub fn get(&self, part: Part) -> Option<&Value> {
        self.manifests.get(part)
    }

    pub fn has_packs(&self) -> bool {
        self.manifests.has_packs()
    }

    /// The game can start: the main game converted (Tekken 3: the packs installed; Tekken Tag Tournament: with its
    /// game root).
    pub fn playable(&self) -> bool {
        match self.game {
            Game::T3 => self.manifests.t3.is_some() && self.installed,
            Game::Ttt => self.manifests.ttt.is_some() && self.ttt_assets,
        }
    }
}

/// The layers the game reads over ROOT, the first first: the extras, then the packs' own files.
fn layers(root: &Path, state: &State) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if state.has_packs() {
        out.push(root.join(EXTRAS));
    }
    for part in Part::LAYERED {
        let assets = dir(part, root).join(kumite_packs::ASSETS);
        if state.get(part).is_some() && assets.is_dir() {
            out.push(assets);
        }
    }
    out
}

/// Tekken Tag Tournament's layers over its game root, the first first: its extras (installed on the games' data as
/// they are now), then the arcade pack's own files.
fn ttt_layers(root: &Path, state: &State) -> Vec<PathBuf> {
    let extras = dir(Part::Ttt, root).join(EXTRAS);
    let arcade = dir(Part::Arcade, root).join(kumite_packs::ASSETS);
    let installed = kumite_packs::ttt_installed(&StdFs(game_root(root, Game::Ttt)), &StdFs(root.to_path_buf()), &StdFs(extras.clone()));
    let arcade_there = state.get(Part::Arcade).is_some() && arcade.is_dir();
    [(extras, installed), (arcade, arcade_there)].into_iter().filter_map(|(p, there)| there.then_some(p)).collect()
}

/// The game reads the main game's layers from now on (Tekken 3: none while the packs are not installed).
pub fn activate(root: &Path) -> State {
    let state = State::of(root, game());
    let layers = match state.game {
        Game::T3 if state.installed => layers(root, &state),
        Game::T3 => Vec::new(),
        Game::Ttt => ttt_layers(root, &state),
    };
    kumite_fs::set_layers(&game_root(root, state.game), layers);
    state
}

/// A job of the Game data screen.
#[derive(Clone, Debug)]
pub enum Job {
    /// Convert a part from the player's copy: a disc image in any format ok_converter_common reads (Tekken 3, Tekken 2:
    /// .cue, .bin, .iso, .chd, .ccd, .mds, .ecm, .pbp; the files it names beside it; or an archive of them, .zip, .7z,
    /// .tar, .gz, .xz, .zst: unpacked into a temporary file in ROOT while it converts) or the Half-Life folder
    /// (Counter-Strike); then the packs installed again.
    Convert(Part, PathBuf),
    /// Remove a pack (or the Tekken 3 pack's files); the rest installed again.
    Remove(Part),
    /// Install the packs (into the extras) as they are.
    Install,
}

/// The message of a job stopped by its cancel flag.
pub const CANCELLED: &str = "cancelled";

/// A `Vfs` that stops working once `cancel` is set: the converter writing into it stops at its next read or write
/// (cancelling a job, the Game data screen's CANCEL).
pub struct Cancellable<'a> {
    pub inner: &'a dyn Vfs,
    pub cancel: &'a AtomicBool,
}

impl Cancellable<'_> {
    fn go(&self) -> Result<()> {
        if self.cancel.load(Ordering::Relaxed) {
            bail!(CANCELLED);
        }
        Ok(())
    }
}

impl Vfs for Cancellable<'_> {
    fn read(&self, path: &str) -> Result<Vec<u8>> {
        self.go()?;
        self.inner.read(path)
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<()> {
        self.go()?;
        self.inner.write(path, data)
    }
    fn exists(&self, path: &str) -> bool {
        self.inner.exists(path)
    }
    fn list(&self, dir: &str) -> Result<Vec<ok_converter_common::io::Entry>> {
        self.go()?;
        self.inner.list(dir)
    }
    fn remove_all(&self, path: &str) -> Result<()> {
        self.go()?;
        self.inner.remove_all(path)
    }
    fn done(&self) -> Option<Vec<String>> {
        self.inner.done()
    }
    fn checkpoint(&self, done: &[String]) -> Result<()> {
        self.inner.checkpoint(done)
    }
}

/// Whether an error is a job's cancelling.
pub fn is_cancelled(e: &anyhow::Error) -> bool {
    e.chain().any(|c| c.to_string() == CANCELLED)
}

/// Runs `job` on the game data in `root`, its progress in `log` (one line at a time); `cancel` set stops it at its
/// next read or write ([`CANCELLED`]): Tekken 3 goes on from its last finished step next time, a pack cut off is
/// removed.
pub fn run(root: &Path, job: &Job, log: &(dyn Fn(&str) + Sync), cancel: &AtomicBool) -> Result<()> {
    std::fs::create_dir_all(root).with_context(|| format!("cannot make {}", root.display()))?;
    let jobs = std::thread::available_parallelism().map_or(1, |n| n.get());
    match job {
        Job::Convert(part, from) => {
            let fs = StdFs(dir(*part, root));
            let out = Cancellable { inner: &fs, cancel };
            let ctx = Ctx { out: &out, log, jobs };
            match part {
                Part::T3 => {
                    let disc = Disc::open_path_in(from, root, log)?;
                    log(&disc.summary());
                    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
                    ok_tekken3_converter::convert_all::convert_resumable(&ctx, disc.data()?, disc.audio(2).map(|t| t as &dyn DiscFile), &now)?;
                }
                // (a pack cut off is no pack: removed, the one before with it)
                Part::T2 => {
                    let disc = Disc::open_path_in(from, root, log)?;
                    log(&disc.summary());
                    let cdda = [2, 3].map(|n| disc.audio(n).map(|t| Box::new(t.clone()) as Box<dyn DiscFile>));
                    ok_tekken2_converter::pack::convert(&ctx, disc.data()?, &cdda).inspect_err(|_| drop(remove(&dir(Part::T2, root))))?;
                }
                Part::Cs => {
                    let (game, _) = ok_hl_cs_converter::game::game_root(from)?;
                    ok_hl_cs_converter::pack::convert(&ctx, &StdFs(game)).inspect_err(|_| drop(remove(&dir(Part::Cs, root))))?;
                }
                // (the ROM set: MAME's .zip, or the folder of its files)
                Part::Arcade => {
                    use ok_tekken3_arcade_converter::roms::{FolderRoms, RomSet, ZipRoms};
                    let folder = StdFs(from.clone());
                    let roms: Box<dyn RomSet + '_> =
                        if from.is_dir() { Box::new(FolderRoms(&folder)) } else { Box::new(ZipRoms::new(std::fs::File::open(from).with_context(|| format!("cannot open {}", from.display()))?)?) };
                    ok_tekken3_arcade_converter::pack::convert(&ctx, roms.as_ref()).inspect_err(|_| drop(remove(&dir(Part::Arcade, root))))?;
                }
                Part::Ttt => {
                    let disc = Disc::open_path_in(from, root, log)?;
                    log(&disc.summary());
                    ok_tekken_tag_converter::pack::convert(&ctx, disc.data()?, false).inspect_err(|_| drop(remove(&dir(Part::Ttt, root))))?;
                }
            }
        }
        Job::Remove(Part::T3) => {
            // (the Tekken 3 pack's files; packs/ stays, the extras go: they need it)
            for dir in ["characters", "stages", "gamedata", "pictures", "audio", "video", EXTRAS, "manifest.json"] {
                remove(&root.join(dir))?;
            }
        }
        Job::Remove(part) => remove(&dir(*part, root))?,
        Job::Install => {}
    }
    // (Tekken Tag Tournament's extras follow either game's pack)
    if matches!(job, Job::Convert(Part::T3 | Part::Ttt, _) | Job::Remove(Part::T3 | Part::Ttt) | Job::Install) {
        install_ttt(root, log, cancel)?;
    }
    // (the arcade and Tekken Tag Tournament packs are not installed: nothing of the extras changes with them)
    if matches!(job, Job::Convert(Part::Arcade | Part::Ttt, _) | Job::Remove(Part::Arcade | Part::Ttt)) {
        return Ok(());
    }
    install(root, log, cancel)
}

/// Installs the packs there are into the extras (made anew), with the whole's manifest; nothing when there is no
/// Tekken 3 pack or no other. Cancelled, the extras are left as they are: installed again next time.
pub fn install(root: &Path, log: &(dyn Fn(&str) + Sync), cancel: &AtomicBool) -> Result<()> {
    let packs: Vec<(Part, StdFs)> = Part::PACKS.into_iter().map(|p| (p, StdFs(dir(p, root)))).collect();
    let packs: Vec<(Part, &dyn Vfs)> = packs.iter().map(|(p, fs)| (*p, fs as &dyn Vfs)).collect();
    let extras = StdFs(root.join(EXTRAS));
    kumite_packs::install(&StdFs(root.to_path_buf()), &packs, &Cancellable { inner: &extras, cancel }, log)
}

/// Installs Tekken Tag Tournament's extras (made anew): what the Tekken 3 pack brings to it, with the whole's
/// manifest; nothing without its game root.
pub fn install_ttt(root: &Path, log: &(dyn Fn(&str) + Sync), cancel: &AtomicBool) -> Result<()> {
    let extras = StdFs(dir(Part::Ttt, root).join(EXTRAS));
    kumite_packs::install_ttt(&StdFs(game_root(root, Game::Ttt)), &StdFs(root.to_path_buf()), &Cancellable { inner: &extras, cancel }, log)
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
    fn no_packs_no_layers_and_nothing_to_install() {
        let dir = std::env::temp_dir().join(format!("ok_packs_none_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let s = State::of(&dir, Game::T3);
        assert!(s.get(Part::T3).is_none() && !s.has_packs() && s.installed && !s.playable());
        assert!(layers(&dir, &s).is_empty());
        install(&dir, &|_| {}, &AtomicBool::new(false)).unwrap();
        assert!(!dir.join(EXTRAS).exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Tekken 3 alone: playable, no layers, as before Tekken Tag Tournament; its pack beside changes neither, and as
    /// the main game it plays once it has its assets/.
    #[test]
    fn tekken_3_alone_and_tekken_tag_beside_it() {
        let root = std::env::temp_dir().join(format!("ok_packs_t3_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(dir(Part::Ttt, &root)).unwrap();
        std::fs::write(root.join("manifest.json"), format!("{{\"format\": {}, \"fingerprint\": \"x\"}}", ok_tekken3_converter::convert_all::FORMAT)).unwrap();
        let t3 = State::of(&root, Game::T3);
        assert!(t3.playable() && t3.installed && !t3.has_packs() && layers(&root, &t3).is_empty() && t3.get(Part::Ttt).is_none());
        let manifest = serde_json::json!({"pack": "ttt", "format": ok_tekken_tag_converter::pack::FORMAT});
        std::fs::write(dir(Part::Ttt, &root).join("manifest.json"), manifest.to_string()).unwrap();
        let both = State::of(&root, Game::T3);
        assert!(both.playable() && both.installed && layers(&root, &both).is_empty() && both.get(Part::Ttt).is_some());
        assert!(!State::of(&root, Game::Ttt).playable());
        std::fs::create_dir_all(game_root(&root, Game::Ttt)).unwrap();
        assert!(State::of(&root, Game::Ttt).playable());
        assert_eq!(game_root(&root, Game::T3), root);
        assert_eq!(game_root(&root, Game::Ttt), root.join("packs/ttt/assets"));
        // Tekken Tag's extras with no Tekken 3 slice: the manifest alone, its fingerprint Tekken Tag's; a layer until
        // the Tekken 3 pack changes; Tekken 3's layers as they were
        let extras = dir(Part::Ttt, &root).join(EXTRAS);
        assert!(ttt_layers(&root, &State::of(&root, Game::Ttt)).is_empty());
        std::fs::write(game_root(&root, Game::Ttt).join("manifest.json"), r#"{"game": "ttt", "format": 8, "fingerprint": "t"}"#).unwrap();
        run(&root, &Job::Install, &|_| {}, &AtomicBool::new(false)).unwrap();
        let names: Vec<_> = std::fs::read_dir(&extras).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(names, ["manifest.json"]);
        assert_eq!(kumite_assets::Assets::new(&extras).unwrap().fingerprint(), "t");
        assert_eq!(ttt_layers(&root, &State::of(&root, Game::Ttt)), vec![extras.clone()]);
        assert!(layers(&root, &State::of(&root, Game::T3)).is_empty() && !root.join(EXTRAS).exists());
        std::fs::write(root.join("manifest.json"), format!("{{\"format\": {}, \"fingerprint\": \"y\"}}", ok_tekken3_converter::convert_all::FORMAT)).unwrap();
        assert!(ttt_layers(&root, &State::of(&root, Game::Ttt)).is_empty());
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// The main game kept and read back; Tekken 3's is no item at all, and its save the file it always was.
    #[test]
    fn the_main_game_round_trips() {
        let _game = crate::save::GAME_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        assert_eq!((game(), crate::save::file()), (Game::T3, crate::save::FILE));
        crate::save::set_main_game(Part::Ttt.kind());
        assert_eq!((game(), crate::save::file()), (Game::Ttt, "openkumite_save_ttt.json"));
        crate::save::set_main_game(Part::T3.kind());
        assert_eq!((game(), crate::save::file()), (Game::T3, crate::save::FILE));
        assert!(!crate::save::save_dir().join("openkumite_game").exists());
    }

    /// The Tekken Tag Tournament pack converted into packs/ttt/ and found, nothing installed with it: with `TTT_DISC`
    /// (its disc image, any format).
    #[test]
    fn the_tekken_tag_pack_lands_in_its_folder() {
        let Ok(disc) = std::env::var("TTT_DISC") else {
            eprintln!("TTT_DISC is not set: skipped");
            return;
        };
        let root = std::env::temp_dir().join(format!("ok_packs_ttt_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        run(&root, &Job::Convert(Part::Ttt, disc.into()), &|_| {}, &AtomicBool::new(false)).unwrap();
        let state = State::of(&root, Game::Ttt);
        assert!(state.get(Part::Ttt).is_some() && state.get(Part::T3).is_none() && state.playable() == state.ttt_assets && !root.join(EXTRAS).exists());
        assert!(dir(Part::Ttt, &root).join("manifest.json").is_file());
        // what the game plays alone, not the converter's research outputs; its extras (no Tekken 3 pack: the manifest)
        let mut names: Vec<_> = std::fs::read_dir(dir(Part::Ttt, &root)).unwrap().map(|e| e.unwrap().file_name()).collect();
        names.sort();
        assert_eq!(names, ["assets", "extras", "manifest.json"]);
        let size = size_of(&dir(Part::Ttt, &root));
        assert!(size <= crate::dataui::TTT_PACK, "the pack ({size} bytes) outgrew its budget: the space a conversion asks for");
        let manifest = |p: PathBuf| serde_json::from_slice::<Value>(&std::fs::read(p.join("manifest.json")).unwrap()).unwrap()["fingerprint"].clone();
        assert_eq!(manifest(dir(Part::Ttt, &root).join(EXTRAS)), manifest(game_root(&root, Game::Ttt)));
        run(&root, &Job::Remove(Part::Ttt), &|_| {}, &AtomicBool::new(false)).unwrap();
        assert!(State::of(&root, Game::Ttt).get(Part::Ttt).is_none());
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// The arcade pack is read as it is, no extras made: with `OK_ARCADE_ROMS` (MAME's tekken3.zip, or its folder).
    #[test]
    fn the_arcade_pack_is_a_layer_of_its_own() {
        let Ok(roms) = std::env::var("OK_ARCADE_ROMS") else {
            eprintln!("OK_ARCADE_ROMS is not set: skipped");
            return;
        };
        let root = std::env::temp_dir().join(format!("ok_packs_arcade_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        run(&root, &Job::Convert(Part::Arcade, roms.into()), &|_| {}, &AtomicBool::new(false)).unwrap();
        let state = State::of(&root, Game::T3);
        assert!(state.get(Part::Arcade).is_some() && !state.has_packs() && state.installed && !root.join(EXTRAS).exists());
        assert_eq!(layers(&root, &state), vec![dir(Part::Arcade, &root).join(kumite_packs::ASSETS)]);
        // (Tekken Tag Tournament the main game: its stages too)
        assert_eq!(ttt_layers(&root, &State::of(&root, Game::Ttt)), vec![dir(Part::Arcade, &root).join(kumite_packs::ASSETS)]);
        let hidden = kumite_assets::Stage::load(&dir(Part::Arcade, &root).join("assets/arcade/hidden.glb")).unwrap();
        assert!(hidden.sky_model && !hidden.meshes.is_empty() && hidden.sky_picture.is_none(), "the hidden arena");
        for n in 0..13 {
            let stage = kumite_assets::Stage::load(&dir(Part::Arcade, &root).join(format!("assets/arcade/stage_{n:02}.glb"))).unwrap();
            assert!(stage.sky_model && stage.sky_cull && !stage.sky.is_empty() && !stage.meshes.is_empty() && stage.floor_period == Some([25.0, 25.0]), "stage {n}");
            // (its sky's panorama: the seventh stage has none)
            assert_eq!(stage.sky_picture.is_some(), n != 6, "stage {n}");
            // what moves: the carousel, the helicopter's rotors, the torches' flame (16 pictures)
            let moving = |spin: bool| stage.sky.iter().filter(|m| m.motion.as_ref().is_some_and(|k| matches!(k, kumite_assets::Motion::Spin { .. }) == spin)).count();
            let expected = match n {
                3 => (1, 0),
                11 => (2, 0),
                6 | 12 => (0, 1),
                _ => (0, 0),
            };
            assert_eq!((moving(true), moving(false)), expected, "stage {n}");
            // (the carousel is set back every 136 frames; the rotors turn on)
            let periods: Vec<_> = stage.sky.iter().filter_map(|m| if let Some(kumite_assets::Motion::Spin { period, .. }) = &m.motion { Some(*period) } else { None }).collect();
            assert_eq!(periods, if n == 3 { vec![Some(136)] } else { vec![None; moving(true)] }, "stage {n}");
        }
        run(&root, &Job::Remove(Part::Arcade), &|_| {}, &AtomicBool::new(false)).unwrap();
        assert!(State::of(&root, Game::T3).get(Part::Arcade).is_none());
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// The Game data screen's jobs from the player's copies to the game's view:
    /// `OK_T3_BIN=... OK_T2_BIN=... OK_CS_DIR=... OK_PACKS_ROOT=<an empty folder> OK_PACKS_FINGERPRINT=<kumite_convert's>
    /// (the disc images in any format)
    /// cargo test --release -p openkumite -- --ignored the_packs_converted_in_the_game`.
    #[test]
    #[ignore]
    fn the_packs_converted_in_the_game() {
        let var = |k: &str| std::env::var(k).unwrap_or_else(|_| panic!("set {k}"));
        let root = PathBuf::from(var("OK_PACKS_ROOT"));
        let log = |l: &str| println!("{l}");
        for (part, from) in [(Part::T3, "OK_T3_BIN"), (Part::T2, "OK_T2_BIN"), (Part::Cs, "OK_CS_DIR")] {
            run(&root, &Job::Convert(part, var(from).into()), &log, &AtomicBool::new(false)).unwrap();
        }
        let state = activate(&root);
        assert!(state.playable() && state.get(Part::T2).is_some() && state.get(Part::Cs).is_some());
        let assets = kumite_assets::Assets::new(&root).unwrap();
        assert_eq!(assets.fingerprint(), var("OK_PACKS_FINGERPRINT"));
        // the layers: a Tekken 2 fighter's model from the extras, its music from the pack, Tekken 3's own from ROOT
        assert!(kumite_fs::resolve(&root.join("characters/model_900.glb")).starts_with(root.join(EXTRAS)));
        let music = kumite_fs::resolve(&root.join("audio/music/t2_00.flac"));
        assert!(music.starts_with(super::dir(Part::T2, &root)), "{}", music.display());
        assert_eq!(kumite_fs::resolve(&root.join("characters/model_000.glb")), root.join("characters/model_000.glb"));
        // removed again: Tekken 3 alone, no layers
        run(&root, &Job::Remove(Part::T2), &log, &AtomicBool::new(false)).unwrap();
        run(&root, &Job::Remove(Part::Cs), &log, &AtomicBool::new(false)).unwrap();
        let state = activate(&root);
        assert!(state.playable() && !state.has_packs() && !root.join(EXTRAS).exists());
        assert_eq!(kumite_assets::Assets::new(&root).unwrap().fingerprint(), "faec270495bc58ba434d29b74cabef14");
    }
}
