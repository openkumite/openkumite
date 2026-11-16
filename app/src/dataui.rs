//! What the Game data screens share (the desktops' datascreen.rs, Android's android.rs): the parts' rows, the job on
//! its thread and its progress. How a part's input is chosen is theirs (a system dialog; Android's picker and copy).

use crate::packs::{self, Job, Part, State};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

/// A job running: its log, and the thread.
pub struct Running {
    pub job: Job,
    log: Arc<Mutex<Vec<String>>>,
    thread: Option<JoinHandle<anyhow::Result<()>>>,
    started: std::time::Instant,
}

/// The game data's state and its job.
pub struct Jobs {
    pub root: PathBuf,
    pub state: State,
    /// Each part's size on disk (`Part::ALL`'s order).
    pub sizes: [u64; 3],
    pub running: Option<Running>,
    /// What went wrong last, or what was done.
    pub message: Option<String>,
    /// Remove asked for this part: waiting for its confirmation.
    pub confirm: Option<Part>,
}

/// A row's button pressed.
pub enum RowAction {
    Choose(Part),
    Remove(Part),
}

impl Jobs {
    /// The game data in `root`; packs found uninstalled (their Tekken 3 pack changed, or a game before this one) are
    /// installed at once.
    pub fn new(root: &Path) -> Self {
        let mut j = Self { root: root.to_path_buf(), state: State::default(), sizes: [0; 3], running: None, message: None, confirm: None };
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
        self.state = State::of(&self.root);
        let root = &self.root;
        self.sizes = Part::ALL.map(|p| match p {
            Part::T3 if self.state.get(Part::T3).is_some() => ["characters", "stages", "gamedata", "pictures", "audio", "video", packs::EXTRAS].iter().map(|d| packs::size_of(&root.join(d))).sum(),
            Part::T3 => 0,
            p => packs::size_of(&packs::dir(p, root)),
        });
    }

    /// Starts `job` on a thread (`scratch`: a folder removed when it ends).
    pub fn start(&mut self, job: Job, scratch: Option<PathBuf>) {
        let log = Arc::new(Mutex::new(Vec::new()));
        let (root, job2, log2, scratch2) = (self.root.clone(), job.clone(), log.clone(), scratch.clone());
        let thread = std::thread::Builder::new().name("game data".into()).spawn(move || {
            let say = move |line: &str| {
                log::info!("{line}");
                if let Ok(mut l) = log2.lock() {
                    l.push(line.to_string());
                }
            };
            let r = packs::run(&root, &job2, &say);
            if let Some(s) = scratch2 {
                let _ = std::fs::remove_dir_all(s);
            }
            r
        });
        match thread {
            Ok(t) => {
                self.message = None;
                self.running = Some(Running { job, log, thread: Some(t), started: std::time::Instant::now() });
            }
            Err(e) => {
                if let Some(s) = scratch {
                    let _ = std::fs::remove_dir_all(s);
                }
                self.message = Some(format!("could not start: {e}"));
            }
        }
    }

    /// A job finished: its result said, the state again. True when one did.
    pub fn follow(&mut self) -> bool {
        let Some(r) = self.running.as_mut() else { return false };
        if !r.thread.as_ref().is_some_and(|t| t.is_finished()) {
            return false;
        }
        let result = r.thread.take().map(|t| t.join());
        let job = r.job.clone();
        self.running = None;
        self.refresh();
        self.message = Some(match result {
            Some(Ok(Ok(()))) => match job {
                Job::Convert(p, _) => format!("{} converted.", p.name()),
                Job::Remove(p) => format!("{} removed.", p.name()),
                Job::Install => "The game data is ready.".into(),
            },
            Some(Ok(Err(e))) => {
                log::error!("{e:#}");
                format!("{e:#}")
            }
            _ => "the conversion stopped unexpectedly".into(),
        });
        true
    }

    /// Every part's row; the button pressed (Remove after its confirmation).
    pub fn rows(&mut self, ui: &mut egui::Ui, pick_folder: bool) -> Option<RowAction> {
        let mut out = None;
        let busy = self.busy();
        for (k, part) in Part::ALL.into_iter().enumerate() {
            let (what, needs) = match part {
                Part::T3 => {
                    ("Tekken 3 (PlayStation, USA, SLUS-00402): the game. Needed.", "its disc image (.cue, .bin, .chd, .iso, .ccd, .mds, .ecm, .pbp, or a .zip, .7z, .tar, .gz, .xz or .zst of it)")
                }
                Part::T2 => (
                    "Tekken 2 (PlayStation, USA v1.1): its fighters join the roster. Optional.",
                    "its disc image (.cue, .bin, .chd, .iso, .ccd, .mds, .ecm, .pbp, or a .zip, .7z, .tar, .gz, .xz or .zst of it)",
                ),
                Part::Cs => ("Counter-Strike (Half-Life): its levels as arenas, two more Tekken Force stages. Optional.", "the Half-Life folder (with cstrike/ and valve/)"),
            };
            let done = self.state.get(part).is_some();
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(egui::RichText::new(part.name()).strong().size(16.0));
                ui.label(what);
                if done {
                    ui.label(egui::RichText::new(format!("Converted: {:.1} GB", self.sizes[k] as f64 / 1e9)).color(egui::Color32::from_rgb(140, 220, 150)));
                    if let Some(note) = self.state.get(part).and_then(kumite_packs::missing_note) {
                        ui.label(egui::RichText::new(note).color(egui::Color32::from_rgb(255, 210, 140)));
                    }
                } else {
                    let how = if pick_folder && part != Part::Cs { format!("the folder with {needs}") } else { needs.to_string() };
                    ui.label(egui::RichText::new(format!("Not converted. Choose {how}.")).weak());
                }
                ui.horizontal(|ui| {
                    if self.confirm == Some(part) {
                        ui.label("Remove it?");
                        if ui.button("Remove").clicked() {
                            out = Some(RowAction::Remove(part));
                            self.confirm = None;
                        }
                        if ui.button("Cancel").clicked() {
                            self.confirm = None;
                        }
                        return;
                    }
                    let pick = match (done, part, pick_folder) {
                        (true, ..) => "Convert again…",
                        (_, Part::Cs, _) | (_, _, true) => "Choose folder…",
                        _ => "Choose disc image…",
                    };
                    if ui.add_enabled(!busy, egui::Button::new(pick).min_size([150.0, 32.0].into())).clicked() {
                        out = Some(RowAction::Choose(part));
                    }
                    if done && ui.add_enabled(!busy, egui::Button::new("Remove").min_size([90.0, 32.0].into())).clicked() {
                        self.confirm = Some(part);
                    }
                });
            });
            ui.add_space(8.0);
        }
        if let Some(RowAction::Remove(p)) = &out {
            self.start(Job::Remove(*p), None);
            out = None;
        }
        out
    }

    /// The job's progress (its last lines), else the last message.
    pub fn progress(&self, ui: &mut egui::Ui) {
        if let Some(run) = &self.running {
            let lines = run.log.lock().map(|l| l.clone()).unwrap_or_default();
            let what = match &run.job {
                Job::Convert(p, _) => format!("Converting {}", p.name()),
                Job::Remove(p) => format!("Removing {}", p.name()),
                Job::Install => "Installing the packs".into(),
            };
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(format!("{what}… {} s", run.started.elapsed().as_secs()));
            });
            for l in lines.iter().rev().take(4).rev() {
                ui.label(egui::RichText::new(l).small().monospace().weak());
            }
        } else if let Some(m) = &self.message {
            ui.colored_label(egui::Color32::from_rgb(255, 190, 140), m);
        }
    }

    /// The heading and the words above the rows.
    pub fn heading(&self, ui: &mut egui::Ui) {
        ui.vertical_centered(|ui| {
            ui.heading(egui::RichText::new("OpenKumite").size(30.0).strong());
            ui.label("Game data");
        });
        ui.add_space(10.0);
        ui.label("OpenKumite plays the games you own. It converts them once, here on this device, and keeps the result; nothing is uploaded.");
        ui.label(egui::RichText::new(format!("Kept in {}", self.root.display())).small().weak());
        ui.add_space(12.0);
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
