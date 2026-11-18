//! The Game data screen's host natively (the desktops' datascreen.rs, Android's android.rs): the game data's state, the
//! job on its thread (cancelled through its output: `packs::Cancellable`), the space check before a conversion, the
//! job's outcome; as the screen ([`crate::gamedata`]) shows them. How a part's input is chosen is the platform's (a
//! system dialog; Android's picker and copy).

use crate::gamedata::progress::Kind;
use crate::gamedata::{Abilities, JobView, Outcome, PartView, Pending, Status};
use crate::packs::{self, Job, Part, State};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

/// A job running: its log, its cancel flag, and the thread.
pub struct Running {
    pub job: Job,
    log: Arc<Mutex<Vec<String>>>,
    cancel: Arc<AtomicBool>,
    thread: Option<JoinHandle<anyhow::Result<()>>>,
    started: web_time::Instant,
}

/// The game data's state and its job.
pub struct Jobs {
    pub root: PathBuf,
    pub state: State,
    /// Each part's size on disk (`Part::ALL`'s order).
    pub sizes: [u64; 5],
    pub running: Option<Running>,
    /// The last job's, until seen.
    pub outcome: Option<Outcome>,
    /// A conversion waiting for the player: the space looks short (and the folder its input was copied to).
    pub pending: Option<(Pending, Job, Option<PathBuf>)>,
    /// What went wrong outside a job (a pick), until the next command.
    pub message: Option<String>,
}

/// A part's job kind.
fn kind(job: &Job) -> Kind {
    match job {
        Job::Convert(p, _) => Kind::Convert(*p),
        Job::Remove(_) => Kind::Remove,
        Job::Install => Kind::Install,
    }
}

/// About how much a part's conversion writes (its pack, and the extras it installs), with an archive's unpacked image.
pub fn needed(part: Part, input: &Path) -> u64 {
    let base: u64 = match part {
        Part::T3 => 1_500_000_000,
        Part::T2 => 1_800_000_000,
        Part::Cs => 200_000_000,
        Part::Arcade => 30_000_000,
        Part::Ttt => 600_000_000,
    };
    let name = input.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    // (the arcade ROM set's .zip is read as it is)
    let unpacked = if part != Part::Arcade && ok_converter_common::disc::is_archive(&name) { 900_000_000 } else { 0 };
    base + unpacked
}

/// The space free where `path` is (its nearest folder that exists).
pub fn free_space(path: &Path) -> Option<u64> {
    let mut p = path;
    loop {
        if p.exists() {
            return fs4::available_space(p).ok();
        }
        p = p.parent()?;
    }
}

impl Jobs {
    /// The game data in `root`; packs found uninstalled (their Tekken 3 pack changed, or a game before this one) are
    /// installed at once.
    pub fn new(root: &Path) -> Self {
        let mut j = Self { root: root.to_path_buf(), state: State::default(), sizes: [0; 5], running: None, outcome: None, pending: None, message: None };
        j.refresh();
        if j.state.get(Part::T3).is_some() && !j.state.installed {
            j.start(Job::Install, None);
        }
        j
    }

    pub fn busy(&self) -> bool {
        self.running.is_some()
    }

    fn refresh(&mut self) {
        self.state = State::of(&self.root, packs::game());
        let root = &self.root;
        self.sizes = Part::ALL.map(|p| match p {
            Part::T3 if self.state.get(Part::T3).is_some() => ["characters", "stages", "gamedata", "pictures", "audio", "video", packs::EXTRAS].iter().map(|d| packs::size_of(&root.join(d))).sum(),
            Part::T3 => 0,
            p => packs::size_of(&packs::dir(p, root)),
        });
    }

    /// A part's input chosen: converted now, or first a question when the space looks short (`scratch`: a folder of the
    /// input's copy, removed when the job ends or the player drops it).
    pub fn convert(&mut self, part: Part, input: PathBuf, scratch: Option<PathBuf>) {
        self.message = None;
        let need = needed(part, &input);
        let free = free_space(&self.root);
        match free {
            Some(free) if free < need => {
                let image = input.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                self.pending = Some((Pending { part, image, needed: need, free }, Job::Convert(part, input), scratch));
            }
            _ => self.start(Job::Convert(part, input), scratch),
        }
    }

    /// The pending conversion started (`go`) or dropped.
    pub fn pending(&mut self, go: bool) {
        if let Some((_, job, scratch)) = self.pending.take() {
            if go {
                self.start(job, scratch);
            } else if let Some(s) = scratch {
                let _ = std::fs::remove_dir_all(s);
            }
        }
    }

    pub fn cancel(&mut self) {
        if let Some(r) = &self.running {
            r.cancel.store(true, Ordering::Relaxed);
        }
    }

    /// Starts `job` on a thread (`scratch`: a folder removed when it ends).
    pub fn start(&mut self, job: Job, scratch: Option<PathBuf>) {
        self.message = None;
        self.outcome = None;
        let log = Arc::new(Mutex::new(Vec::new()));
        let cancel = Arc::new(AtomicBool::new(false));
        let (root, job2, log2, cancel2, scratch2) = (self.root.clone(), job.clone(), log.clone(), cancel.clone(), scratch.clone());
        let thread = std::thread::Builder::new().name("game data".into()).spawn(move || {
            let say = move |line: &str| {
                log::info!("{line}");
                if let Ok(mut l) = log2.lock() {
                    l.push(line.to_string());
                }
            };
            let r = packs::run(&root, &job2, &say, &cancel2);
            if let Some(s) = scratch2 {
                let _ = std::fs::remove_dir_all(s);
            }
            r
        });
        match thread {
            Ok(t) => self.running = Some(Running { job, log, cancel, thread: Some(t), started: web_time::Instant::now() }),
            Err(e) => {
                if let Some(s) = scratch {
                    let _ = std::fs::remove_dir_all(s);
                }
                self.message = Some(format!("The job could not start: {e}"));
            }
        }
    }

    /// A job finished: its outcome kept, the state again. True when one did.
    pub fn follow(&mut self) -> bool {
        let Some(r) = self.running.as_mut() else { return false };
        if !r.thread.as_ref().is_some_and(|t| t.is_finished()) {
            return false;
        }
        let result = r.thread.take().map(|t| t.join());
        let job = r.job.clone();
        let log = r.log.lock().map(|l| l.clone()).unwrap_or_default();
        self.running = None;
        self.refresh();
        let note = |p: Part| self.state.get(p).and_then(crate::gamedata::missing_note);
        let (ok, cancelled, message) = match result {
            Some(Ok(Ok(()))) => match &job {
                // (removing and installing say nothing when they work: the screen shows the state)
                Job::Remove(_) | Job::Install => return true,
                Job::Convert(p, _) => {
                    let done = match p {
                        Part::T3 => "Tekken 3 is converted: the game can start.",
                        Part::T2 => "Tekken 2 is converted: its fighters are in the roster.",
                        Part::Cs => "Counter-Strike is converted: its levels are arenas now.",
                        Part::Arcade => "The arcade stages are converted: they replace the PlayStation ones (OPTION, GRAPHICS, STAGES chooses).",
                        Part::Ttt => "Tekken Tag Tournament is converted.",
                    };
                    (true, false, note(*p).map_or(done.to_string(), |n| format!("{done} {n}")))
                }
            },
            Some(Ok(Err(e))) if packs::is_cancelled(&e) => (
                false,
                true,
                match &job {
                    Job::Convert(p, _) => format!("{} was not converted. Convert it again whenever you like.", p.name()),
                    _ => "Stopped. The game data is installed again the next time OpenKumite starts.".into(),
                },
            ),
            Some(Ok(Err(e))) => {
                log::error!("{e:#}");
                (false, false, format!("{e:#}"))
            }
            _ => (false, false, "The conversion stopped unexpectedly.".into()),
        };
        self.outcome = Some(Outcome { kind: kind(&job), ok, cancelled, message, log });
        true
    }

    /// The state as the screen shows it.
    pub fn status(&self, can: Abilities) -> Status {
        let parts = Part::ALL.map(|p| {
            let m = self.state.get(p);
            let k = Part::ALL.iter().position(|q| *q == p).unwrap_or(0);
            PartView { converted: m.is_some(), size: self.sizes[k], note: m.and_then(crate::gamedata::missing_note) }
        });
        let job = self.running.as_ref().map(|r| JobView {
            kind: kind(&r.job),
            seconds: r.started.elapsed().as_secs_f64(),
            log: r.log.lock().map(|l| l.clone()).unwrap_or_default(),
            cancellable: !matches!(r.job, Job::Remove(_)),
            cancelling: r.cancel.load(Ordering::Relaxed),
        });
        Status {
            parts,
            playable: self.state.playable(),
            main: self.state.game.part(),
            loading: false,
            job,
            outcome: self.outcome.clone(),
            pending: self.pending.as_ref().map(|p| p.0.clone()),
            picking: None,
            copy: None,
            dropped: false,
            hovering: false,
            message: self.message.clone(),
            can,
        }
    }

    /// The screen's commands every platform does alike; the others back (picking, playing, quitting: the
    /// platform's).
    pub fn command(&mut self, c: crate::gamedata::Command) -> Option<crate::gamedata::Command> {
        use crate::gamedata::Command as C;
        match c {
            C::Remove(p) if !self.busy() => self.start(Job::Remove(p), None),
            C::Cancel => self.cancel(),
            C::ConvertAnyway => self.pending(true),
            C::DropPending => self.pending(false),
            C::Seen => self.outcome = None,
            // (the other game started as a conversion's Play starts it: when it can)
            C::MainGame(p) if !self.busy() => {
                crate::save::set_main_game(p.kind());
                self.refresh();
                return Some(C::Play);
            }
            other => return Some(other),
        }
        None
    }
}

#[cfg_attr(not(target_os = "android"), allow(dead_code))]
/// In a copied folder (Android's picker; a .zip unpacked), the input for `part`: the disc image (a sheet, .cue, .ccd or
/// .mds, else a .chd or .pbp, else the image named Track 1, else the largest: ok_converter_common's `main_image`; the
/// folder's own files first, then its sub-folders'), or the Half-Life folder (the copy itself: the converter finds
/// cstrike/ in it, or takes it as a cstrike folder).
pub fn input_in(dir: &Path, part: Part) -> anyhow::Result<PathBuf> {
    if part == Part::Cs {
        return Ok(dir.to_path_buf());
    }
    // (the arcade ROM set: its .zip when the folder holds one, else the folder of its files)
    if part == Part::Arcade {
        let mut zips: Vec<PathBuf> = std::fs::read_dir(dir)?.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("zip"))).collect();
        zips.sort();
        return Ok(zips.into_iter().next().unwrap_or_else(|| dir.to_path_buf()));
    }
    let mut dirs = vec![dir.to_path_buf()];
    let mut k = 0;
    while let Some(d) = dirs.get(k).cloned() {
        if let Some(name) = ok_converter_common::disc::main_image(&ok_converter_common::disc::StdFolder(d.clone())) {
            return Ok(d.join(name));
        }
        let mut subs: Vec<PathBuf> = std::fs::read_dir(&d)?.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
        subs.sort();
        dirs.extend(subs);
        k += 1;
    }
    anyhow::bail!("There is no disc image in that folder (.cue, .bin, .chd, .iso, .ccd, .mds, .ecm, .pbp, or an archive of it). Choose the folder that holds {}'s disc image.", part.name())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A conversion cancelled stops within moments and says so (`OK_T3_BIN=<a disc image> cargo test --release -p
    /// openkumite -- --ignored a_conversion_cancelled`).
    #[test]
    #[ignore]
    fn a_conversion_cancelled_stops_and_says_so() {
        let root = std::env::temp_dir().join(format!("ok_cancel_{}", std::process::id()));
        let mut jobs = Jobs::new(&root);
        jobs.start(Job::Convert(Part::T3, std::env::var("OK_T3_BIN").unwrap().into()), None);
        std::thread::sleep(std::time::Duration::from_secs(3));
        assert!(jobs.busy());
        jobs.cancel();
        let asked = std::time::Instant::now();
        while !jobs.follow() {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        let o = jobs.outcome.clone().unwrap();
        println!("stopped {:.2} s after cancel: {}", asked.elapsed().as_secs_f64(), o.message);
        assert!(o.cancelled && !o.ok && asked.elapsed().as_secs() < 10);
        assert!(!jobs.state.playable());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn the_disc_image_in_a_copied_folder() {
        let dir = std::env::temp_dir().join(format!("ok_dataui_input_{}", std::process::id()));
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        assert!(input_in(&dir, Part::T3).is_err());
        std::fs::write(dir.join("sub/game.chd"), vec![0u8; 10]).unwrap();
        assert_eq!(input_in(&dir, Part::T2).unwrap(), dir.join("sub/game.chd"), "in a sub-folder");
        std::fs::write(dir.join("other.BIN"), vec![0u8; 5]).unwrap();
        std::fs::write(dir.join("big.bin"), vec![0u8; 9]).unwrap();
        assert_eq!(input_in(&dir, Part::T2).unwrap(), dir.join("big.bin"), "the folder's own, the largest");
        std::fs::write(dir.join("Game (Track 1).bin"), vec![0u8; 1]).unwrap();
        assert_eq!(input_in(&dir, Part::T3).unwrap(), dir.join("Game (Track 1).bin"), "track 1 first");
        std::fs::write(dir.join("Game.cue"), b"FILE").unwrap();
        assert_eq!(input_in(&dir, Part::T3).unwrap(), dir.join("Game.cue"), "the sheet first");
        assert_eq!(input_in(&dir, Part::Cs).unwrap(), dir);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
