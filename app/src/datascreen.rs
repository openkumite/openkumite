//! The Game data screen on the desktops: the game converts the player's own copies itself (packs.rs). Tekken 3's
//! disc image is needed; Tekken 2's disc image (its fighters) and the Counter-Strike game (its levels as arenas) can
//! be added, replaced or removed. Shown when there is no game data yet (or of an older format, or the packs need
//! installing again), and from the title screen's Game data button. The conversions run on a thread; the window
//! stays live.
//!
//! [`Root`] is the program's event loop: the screen or the game, handing the window and the renderer from one to the
//! other (winit has one event loop a process).

use crate::debug::EguiLayer;
use crate::packs::{self, Job, Part, State};
use crate::App;
use kumite_render::Renderer;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow};
use winit::window::{Window, WindowId};

pub struct Root {
    /// The game data's folder (the Tekken 3 pack, the packs beside it).
    root: PathBuf,
    args: Vec<String>,
    now: Now,
}

enum Now {
    Data(Box<Data>),
    Game(Box<App>),
}

impl Root {
    /// The game straight away.
    pub fn game(root: PathBuf, args: Vec<String>, app: App) -> Self {
        Self { root, args, now: Now::Game(Box::new(app)) }
    }

    /// The Game data screen first.
    pub fn data(root: PathBuf, args: Vec<String>) -> Self {
        let d = Data::new(&root, None, None);
        Self { root, args, now: Now::Data(Box::new(d)) }
    }

    /// From one to the other: the game asked for the screen; the screen's Play.
    fn switch(&mut self, event_loop: &ActiveEventLoop) {
        match &mut self.now {
            Now::Game(app) if app.game_data => {
                let d = Data::new(&self.root, app.window.take(), app.renderer.take());
                self.now = Now::Data(Box::new(d));
            }
            Now::Data(d) if d.play => {
                d.play = false;
                packs::activate(&self.root);
                match crate::new_app(self.root.clone(), self.args.clone()) {
                    Ok(Some(mut app)) => {
                        if let (Some(w), Some(r)) = (d.window.take(), d.renderer.take()) {
                            app.start(event_loop, w, r);
                        }
                        self.now = Now::Game(Box::new(app));
                    }
                    Ok(None) => event_loop.exit(),
                    Err(e) => {
                        log::error!("{e:#}");
                        d.message = Some(format!("{e:#}"));
                    }
                }
            }
            _ => {}
        }
    }
}

impl ApplicationHandler for Root {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        match &mut self.now {
            Now::Game(app) => app.resumed(event_loop),
            Now::Data(d) => d.resumed(event_loop),
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        match &mut self.now {
            Now::Game(app) => app.window_event(event_loop, id, event),
            Now::Data(d) => d.window_event(event_loop, &self.root, event),
        }
        self.switch(event_loop);
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        match &mut self.now {
            Now::Game(app) => app.about_to_wait(event_loop),
            Now::Data(d) => {
                event_loop.set_control_flow(ControlFlow::wait_duration(std::time::Duration::from_millis(50)));
                if let Some(w) = &d.window {
                    w.request_redraw();
                }
            }
        }
        self.switch(event_loop);
    }

    fn exiting(&mut self, event_loop: &ActiveEventLoop) {
        if let Now::Game(app) = &mut self.now {
            app.exiting(event_loop);
        }
    }
}

/// A job running: its log, and the thread.
struct Running {
    job: Job,
    log: Arc<Mutex<Vec<String>>>,
    thread: Option<JoinHandle<anyhow::Result<()>>>,
    started: std::time::Instant,
}

struct Data {
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
    egui: Option<EguiLayer>,
    state: State,
    /// Each part's size on disk.
    sizes: [u64; 3],
    running: Option<Running>,
    /// What went wrong last, or what was done.
    message: Option<String>,
    /// Remove asked for this part: waiting for its confirmation.
    confirm: Option<Part>,
    /// Start the game.
    play: bool,
}

impl Data {
    fn new(root: &Path, window: Option<Arc<Window>>, renderer: Option<Renderer>) -> Self {
        let egui = renderer.as_ref().map(|r| EguiLayer::new(r.device(), r.overlay_format(), window.as_deref()));
        let mut d = Self { window, renderer, egui, state: State::default(), sizes: [0; 3], running: None, message: None, confirm: None, play: false };
        d.refresh(root);
        // (packs whose Tekken 3 pack changed, or installed by an older game: installed again)
        if d.state.t3.is_some() && !d.state.installed {
            d.start(root, Job::Install);
        }
        d
    }

    fn refresh(&mut self, root: &Path) {
        self.state = State::of(root);
        self.sizes = Part::ALL.map(|p| match p {
            Part::T3 if self.state.t3.is_some() => {
                ["characters", "stages", "gamedata", "pictures", "audio", "video"].iter().map(|d| packs::size_of(&root.join(d))).sum::<u64>() + packs::size_of(&root.join(packs::EXTRAS))
            }
            Part::T3 => 0,
            p => packs::size_of(&p.dir(root)),
        });
    }

    fn start(&mut self, root: &Path, job: Job) {
        let log = Arc::new(Mutex::new(Vec::new()));
        let (root, job2, log2) = (root.to_path_buf(), job.clone(), log.clone());
        let thread = std::thread::Builder::new().name("game data".into()).spawn(move || {
            let say = move |line: &str| {
                log::info!("{line}");
                if let Ok(mut l) = log2.lock() {
                    l.push(line.to_string());
                }
            };
            packs::run(&root, &job2, &say)
        });
        match thread {
            Ok(t) => {
                self.message = None;
                self.running = Some(Running { job, log, thread: Some(t), started: std::time::Instant::now() });
            }
            Err(e) => self.message = Some(format!("could not start: {e}")),
        }
    }

    /// A job finished: its result said, the state again.
    fn follow(&mut self, root: &Path) {
        let Some(r) = self.running.as_mut() else { return };
        if !r.thread.as_ref().is_some_and(|t| t.is_finished()) {
            return;
        }
        let result = r.thread.take().map(|t| t.join());
        let job = r.job.clone();
        self.running = None;
        self.refresh(root);
        self.message = Some(match result {
            Some(Ok(Ok(()))) => match job {
                Job::Convert(p, _) => format!("{} converted.", p.name()),
                Job::Remove(p) => format!("{} removed.", p.name()),
                Job::Install => "The game data is ready.".into(),
            },
            Some(Ok(Err(e))) => format!("{e:#}"),
            _ => "the conversion stopped unexpectedly".into(),
        });
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attrs = Window::default_attributes().with_title("OpenKumite").with_inner_size(winit::dpi::LogicalSize::new(960.0, 640.0));
        let window = match event_loop.create_window(attrs) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                log::error!("no window: {e}");
                event_loop.exit();
                return;
            }
        };
        let size = window.inner_size();
        match Renderer::new(window.clone(), size.width, size.height) {
            Ok(r) => {
                self.egui = Some(EguiLayer::new(r.device(), r.overlay_format(), Some(&window)));
                self.renderer = Some(r);
                self.window = Some(window);
            }
            Err(e) => {
                log::error!("no renderer: {e:#}");
                event_loop.exit();
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, root: &Path, event: WindowEvent) {
        if let (Some(e), Some(w)) = (&mut self.egui, &self.window) {
            e.on_event(w, &event);
        }
        match event {
            WindowEvent::Resized(s) => {
                if let Some(r) = self.renderer.as_mut() {
                    r.resize(s.width, s.height);
                }
            }
            // (not while converting: the thread would be cut off mid-write; it resumes where it can)
            WindowEvent::CloseRequested if self.running.is_none() => event_loop.exit(),
            WindowEvent::RedrawRequested => self.redraw(event_loop, root),
            _ => {}
        }
    }

    fn redraw(&mut self, event_loop: &ActiveEventLoop, root: &Path) {
        self.follow(root);
        let (Some(egui), Some(r)) = (self.egui.as_mut(), self.renderer.as_mut()) else { return };
        let mut asked: Option<Job> = None;
        let mut quit = false;
        let busy = self.running.is_some();
        let state = &self.state;
        let sizes = self.sizes;
        let mut frame = egui.run(self.window.as_deref(), r.size(), |ctx| {
            let screen = ctx.content_rect();
            ctx.layer_painter(egui::LayerId::background()).rect_filled(screen, 0.0, egui::Color32::from_rgb(12, 13, 17));
            egui::Area::new(egui::Id::new("game_data")).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
                ui.set_width((screen.width() - 48.0).min(680.0));
                ui.vertical_centered(|ui| {
                    ui.heading(egui::RichText::new("OpenKumite").size(30.0).strong());
                    ui.label("Game data");
                });
                ui.add_space(10.0);
                ui.label("OpenKumite plays the games you own. It converts them once, here on this computer, and keeps the result; nothing is uploaded.");
                ui.label(egui::RichText::new(format!("Kept in {}", root.display())).small().weak());
                ui.add_space(12.0);
                for part in Part::ALL {
                    part_row(ui, part, state.get(part), sizes, busy, &mut self.confirm, &mut asked);
                    ui.add_space(8.0);
                }
                ui.add_space(6.0);
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
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    let play = ui.add_enabled(!busy && state.playable(), egui::Button::new(egui::RichText::new("Play").size(18.0)).min_size([140.0, 40.0].into()));
                    if play.clicked() {
                        asked = None;
                        self.play = true;
                    }
                    if ui.add_enabled(!busy, egui::Button::new("Quit").min_size([90.0, 40.0].into())).clicked() {
                        quit = true;
                    }
                });
            });
        });
        let cam = kumite_render::Camera::new(glam::Vec3::Z, glam::Vec3::ZERO, 1.0);
        if let Err(e) = r.render_with(&cam, &[], &[], &[], None, &[], Some(&mut frame)) {
            log::error!("render: {e:#}");
        }
        if quit {
            event_loop.exit();
        }
        if let Some(job) = asked {
            self.start(root, job);
        }
    }
}

/// A part's row: what it is, its state, its buttons (a pick starts its conversion; Remove asks first).
fn part_row(ui: &mut egui::Ui, part: Part, manifest: Option<&serde_json::Value>, sizes: [u64; 3], busy: bool, confirm: &mut Option<Part>, asked: &mut Option<Job>) {
    let (what, needs) = match part {
        Part::T3 => ("Tekken 3 (PlayStation, USA, SLUS-00402): the game. Needed.", "its disc image, Track 1 (.bin; Track 2 beside it)"),
        Part::T2 => ("Tekken 2 (PlayStation, USA v1.1): its fighters join the roster. Optional.", "its disc image, Track 1 (.bin)"),
        Part::Cs => ("Counter-Strike (Half-Life): its levels as arenas, two more Tekken Force stages. Optional.", "the Half-Life folder (with cstrike/ and valve/)"),
    };
    let size = sizes[Part::ALL.iter().position(|p| *p == part).unwrap_or(0)];
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(egui::RichText::new(part.name()).strong().size(16.0));
        ui.label(what);
        match manifest {
            Some(_) => ui.label(egui::RichText::new(format!("Converted: {:.1} GB", size as f64 / 1e9)).color(egui::Color32::from_rgb(140, 220, 150))),
            None => ui.label(egui::RichText::new(format!("Not converted. Choose {needs}.")).weak()),
        };
        ui.horizontal(|ui| {
            if *confirm == Some(part) {
                ui.label("Remove it?");
                if ui.button("Remove").clicked() {
                    *asked = Some(Job::Remove(part));
                    *confirm = None;
                }
                if ui.button("Cancel").clicked() {
                    *confirm = None;
                }
                return;
            }
            let pick = if manifest.is_some() {
                "Convert again…"
            } else if part == Part::Cs {
                "Choose folder…"
            } else {
                "Choose disc image…"
            };
            if ui.add_enabled(!busy, egui::Button::new(pick)).clicked() {
                if let Some(path) = choose(part) {
                    *asked = Some(Job::Convert(part, path));
                }
            }
            if manifest.is_some() && ui.add_enabled(!busy, egui::Button::new("Remove")).clicked() {
                *confirm = Some(part);
            }
        });
    });
}

/// The system's dialog for the part's input: a disc image, or the game folder.
fn choose(part: Part) -> Option<PathBuf> {
    let title = format!("{}: {}", part.name(), if part == Part::Cs { "the Half-Life folder" } else { "the disc image's Track 1" });
    let d = rfd::FileDialog::new().set_title(title);
    match part {
        Part::Cs => d.pick_folder(),
        _ => d.add_filter("Disc image (raw .bin)", &["bin", "BIN"]).pick_file(),
    }
}

/// The title screen's Game data button (top right); pressed.
pub fn game_data_button(ctx: &egui::Context) -> bool {
    let area = egui::Area::new(egui::Id::new("game_data_button")).anchor(egui::Align2::RIGHT_TOP, [-10.0, 10.0]);
    area.show(ctx, |ui| ui.button("Game data").clicked()).inner
}
