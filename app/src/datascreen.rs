//! The Game data screen on the desktops: the game converts the player's own copies itself (packs.rs). Tekken 3's
//! disc image is needed; Tekken 2's disc image (its fighters) and the Counter-Strike game (its levels as arenas) can
//! be added, replaced or removed. Shown when there is no game data yet (or of an older format, or the packs need
//! installing again), and from the title screen's Game data button. The conversions run on a thread; the window
//! stays live.
//!
//! [`Root`] is the program's event loop: the screen or the game, handing the window and the renderer from one to the
//! other (winit has one event loop a process).

use crate::dataui::{Jobs, RowAction};
use crate::debug::EguiLayer;
use crate::packs::{self, Job, Part};
use crate::App;
use kumite_render::Renderer;
use std::path::{Path, PathBuf};
use std::sync::Arc;
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
                        d.jobs.message = Some(format!("{e:#}"));
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
            Now::Data(d) => d.window_event(event_loop, event),
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

struct Data {
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
    egui: Option<EguiLayer>,
    jobs: Jobs,
    /// Start the game.
    play: bool,
}

impl Data {
    fn new(root: &Path, window: Option<Arc<Window>>, renderer: Option<Renderer>) -> Self {
        let egui = renderer.as_ref().map(|r| EguiLayer::new(r.device(), r.overlay_format(), window.as_deref()));
        Self { window, renderer, egui, jobs: Jobs::new(root), play: false }
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attrs = Window::default_attributes().with_title("OpenKumite").with_inner_size(winit::dpi::LogicalSize::new(960.0, 680.0));
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

    fn window_event(&mut self, event_loop: &ActiveEventLoop, event: WindowEvent) {
        if let (Some(e), Some(w)) = (&mut self.egui, &self.window) {
            e.on_event(w, &event);
        }
        match event {
            WindowEvent::Resized(s) => {
                if let Some(r) = self.renderer.as_mut() {
                    r.resize(s.width, s.height);
                }
            }
            // (not while converting: the thread would be cut off mid-write)
            WindowEvent::CloseRequested if !self.jobs.busy() => event_loop.exit(),
            WindowEvent::RedrawRequested => self.redraw(event_loop),
            _ => {}
        }
    }

    fn redraw(&mut self, event_loop: &ActiveEventLoop) {
        self.jobs.follow();
        let (Some(egui), Some(r)) = (self.egui.as_mut(), self.renderer.as_mut()) else { return };
        let (mut action, mut quit, mut play) = (None, false, false);
        let jobs = &mut self.jobs;
        let mut frame = egui.run(self.window.as_deref(), r.size(), |ctx| {
            let screen = ctx.content_rect();
            ctx.layer_painter(egui::LayerId::background()).rect_filled(screen, 0.0, egui::Color32::from_rgb(12, 13, 17));
            egui::Area::new(egui::Id::new("game_data")).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
                ui.set_width((screen.width() - 48.0).min(680.0));
                jobs.heading(ui);
                action = jobs.rows(ui, false);
                ui.add_space(6.0);
                jobs.progress(ui);
                ui.add_space(12.0);
                let busy = jobs.busy();
                ui.horizontal(|ui| {
                    let b = egui::Button::new(egui::RichText::new("Play").size(18.0)).min_size([140.0, 40.0].into());
                    play = ui.add_enabled(!busy && jobs.state.playable(), b).clicked();
                    quit = ui.add_enabled(!busy, egui::Button::new("Quit").min_size([90.0, 40.0].into())).clicked();
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
        self.play |= play;
        if let Some(RowAction::Choose(part)) = action {
            if let Some(path) = choose(part) {
                self.jobs.start(Job::Convert(part, path), None);
            }
        }
    }
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
