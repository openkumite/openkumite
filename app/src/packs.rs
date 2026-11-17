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
//! ```
//!
//! The game reads the layers ([`activate`]: kumite_fs): a file of ROOT from `extras/`, else a pack's `assets/` (the
//! Tekken 2 pack's music and endings), else ROOT. So the extras' manifest.json is the one the game reads: the Tekken
//! 3 pack's, with the packs and the fingerprint of the whole (online play compares it), and what it was installed
//! from (`installed`: the Tekken 3 pack's fingerprint and the packs' sources); when that changes, the packs are
//! installed again. The fingerprint covers characters/, stages/ and gamedata/ only, so packs/ and extras/ are no
//! part of the Tekken 3 pack's own.

use anyhow::{bail, Context, Result};
pub use kumite_packs::Part;
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

/// What the game data's parts are now.
#[derive(Clone, Debug, Default)]
pub struct State {
    /// Each part's manifest (None: not converted, or another format: converted again).
    pub manifests: kumite_packs::Manifests,
    /// The packs there are installed, as they are (the extras are theirs).
    pub installed: bool,
}

impl State {
    pub fn of(root: &Path) -> Self {
        let m = |p: Part| p.manifest(&StdFs(dir(p, root)));
        let manifests = kumite_packs::Manifests { t3: m(Part::T3), t2: m(Part::T2), cs: m(Part::Cs), arcade: m(Part::Arcade) };
        let installed = manifests.installed(&StdFs(root.join(EXTRAS)));
        Self { manifests, installed }
    }

    pub fn get(&self, part: Part) -> Option<&Value> {
        self.manifests.get(part)
    }

    pub fn has_packs(&self) -> bool {
        self.manifests.has_packs()
    }

    /// The game can start: Tekken 3 converted, the packs installed.
    pub fn playable(&self) -> bool {
        self.manifests.t3.is_some() && self.installed
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

/// The game reads ROOT's layers from now on (none while the packs are not installed: Tekken 3 alone).
pub fn activate(root: &Path) -> State {
    let state = State::of(root);
    kumite_fs::set_layers(root, if state.installed { layers(root, &state) } else { Vec::new() });
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
    // (the arcade pack is not installed: nothing of the extras changes with it)
    if matches!(job, Job::Convert(Part::Arcade, _) | Job::Remove(Part::Arcade)) {
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
        let s = State::of(&dir);
        assert!(s.get(Part::T3).is_none() && !s.has_packs() && s.installed && !s.playable());
        assert!(layers(&dir, &s).is_empty());
        install(&dir, &|_| {}, &AtomicBool::new(false)).unwrap();
        assert!(!dir.join(EXTRAS).exists());
        std::fs::remove_dir_all(&dir).unwrap();
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
        let state = State::of(&root);
        assert!(state.get(Part::Arcade).is_some() && !state.has_packs() && state.installed && !root.join(EXTRAS).exists());
        assert_eq!(layers(&root, &state), vec![dir(Part::Arcade, &root).join(kumite_packs::ASSETS)]);
        let hidden = kumite_assets::Stage::load(&dir(Part::Arcade, &root).join("assets/arcade/hidden.glb")).unwrap();
        assert!(hidden.sky_model && !hidden.meshes.is_empty() && hidden.sky_picture.is_none(), "the hidden arena");
        for n in 0..13 {
            let stage = kumite_assets::Stage::load(&dir(Part::Arcade, &root).join(format!("assets/arcade/stage_{n:02}.glb"))).unwrap();
            assert!(stage.sky_model && stage.sky_cull && !stage.sky.is_empty() && !stage.meshes.is_empty() && stage.floor_period == Some([25.0, 25.0]), "stage {n}");
            // (its sky's panorama: the seventh stage has none)
            assert_eq!(stage.sky_picture.is_some(), n != 6, "stage {n}");
            // what moves: the carousel, the helicopter's rotors, the torches' flame (16 pictures)
            let moving = |spin: bool| stage.sky.iter().filter(|m| m.motion.is_some_and(|k| matches!(k, kumite_assets::Motion::Spin { .. }) == spin)).count();
            let expected = match n {
                3 => (1, 0),
                11 => (2, 0),
                6 | 12 => (0, 1),
                _ => (0, 0),
            };
            assert_eq!((moving(true), moving(false)), expected, "stage {n}");
        }
        run(&root, &Job::Remove(Part::Arcade), &|_| {}, &AtomicBool::new(false)).unwrap();
        assert!(State::of(&root).get(Part::Arcade).is_none());
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
