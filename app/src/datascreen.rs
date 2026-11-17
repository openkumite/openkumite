//! The Game data screen (crate::gamedata) on the desktops and in a browser: the game converts the player's own
//! copies itself. Tekken 3's disc image is needed; Tekken 2's disc image (its fighters) and the Counter-Strike game
//! (its levels as arenas) can be added, replaced or removed. Shown when there is no game data yet (or of an older
//! format, or the packs need installing again), and from the title screen's Game data button.
//!
//! Its host: natively the jobs on their thread (dataui.rs) and the system's file dialogs; in a browser the page
//! (gamedata/web.rs: web/index.html picks the files and runs the converter in a worker). [`Root`] is the program's
//! event loop: the screen or the game, handing the window and the renderer from one to the other (winit has one
//! event loop a process).

#[cfg(not(target_arch = "wasm32"))]
use crate::dataui::Jobs;
use crate::debug::EguiLayer;
#[cfg(not(target_arch = "wasm32"))]
use crate::gamedata::{Abilities, Part};
use crate::gamedata::{Command, Screen, Status};
#[cfg(not(target_arch = "wasm32"))]
use crate::packs;
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
                // (a browser's game data changed: the page starts again on it)
                #[cfg(target_arch = "wasm32")]
                if d.host.play_reloads() {
                    crate::gamedata::web::WebHost::reload();
                    return;
                }
                #[cfg(target_arch = "wasm32")]
                d.host.leave();
                #[cfg(not(target_arch = "wasm32"))]
                packs::activate(&self.root);
                match crate::new_app(self.root.clone(), self.args.clone()) {
                    Ok(Some(mut app)) => {
                        if let (Some(w), Some(r)) = (d.window.take(), d.renderer.take()) {
                            // (the game's own name back: the screen's was "game data")
                            w.set_title("OpenKumite");
                            app.start(event_loop, w, r);
                        }
                        self.now = Now::Game(Box::new(app));
                    }
                    Ok(None) => event_loop.exit(),
                    Err(e) => {
                        log::error!("{e:#}");
                        d.message = Some(format!("The game did not start: {e:#}"));
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
                d.arrived();
                event_loop.set_control_flow(ControlFlow::wait_duration(std::time::Duration::from_millis(33)));
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

/// The screen's host: the jobs natively, the page in a browser.
#[cfg(not(target_arch = "wasm32"))]
type Host = Jobs;
#[cfg(target_arch = "wasm32")]
type Host = crate::gamedata::web::WebHost;

struct Data {
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
    /// A browser's renderer, on its way (a browser cannot wait for the GPU).
    #[cfg(target_arch = "wasm32")]
    pending: std::rc::Rc<std::cell::RefCell<Option<Renderer>>>,
    /// The page's size the canvas was last fitted to (CSS pixels), and the frames since (asked again now and then until
    /// the canvas has it).
    #[cfg(target_arch = "wasm32")]
    page: [f64; 2],
    #[cfg(target_arch = "wasm32")]
    page_tries: u32,
    egui: Option<EguiLayer>,
    host: Host,
    screen: Screen,
    /// Why the game did not start (the screen says it).
    message: Option<String>,
    /// Start the game.
    play: bool,
    /// The window closed during a job: it is cancelled, the program ends when it has stopped.
    quit_after: bool,
}

/// What the desktops can do on the Game data screen.
#[cfg(not(target_arch = "wasm32"))]
const CAN: Abilities = Abilities { quit: true, folders: false, copy: true, drop: true, import: None };

impl Data {
    fn new(root: &Path, window: Option<Arc<Window>>, renderer: Option<Renderer>) -> Self {
        let egui = renderer.as_ref().map(|r| EguiLayer::new(r.device(), r.overlay_format(), window.as_deref()));
        #[cfg(not(target_arch = "wasm32"))]
        let host = Jobs::new(root);
        #[cfg(target_arch = "wasm32")]
        let host = {
            let _ = root;
            crate::gamedata::web::WebHost::new()
        };
        if let Some(w) = &window {
            w.set_title("OpenKumite: game data");
        }
        Self {
            window,
            renderer,
            #[cfg(target_arch = "wasm32")]
            pending: Default::default(),
            #[cfg(target_arch = "wasm32")]
            page: [0.0; 2],
            #[cfg(target_arch = "wasm32")]
            page_tries: 0,
            egui,
            host,
            screen: Screen::new().with_gamepads(),
            message: None,
            play: false,
            quit_after: false,
        }
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attrs = Window::default_attributes().with_title("OpenKumite: game data").with_inner_size(winit::dpi::LogicalSize::new(960.0, 680.0));
        // in a browser: a canvas added to the page
        #[cfg(target_arch = "wasm32")]
        let attrs = winit::platform::web::WindowAttributesExtWebSys::with_append(attrs, true);
        let window = match event_loop.create_window(attrs) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                log::error!("no window: {e}");
                event_loop.exit();
                return;
            }
        };
        let size = window.inner_size();
        #[cfg(not(target_arch = "wasm32"))]
        match Renderer::new(window.clone(), size.width, size.height) {
            Ok(r) => {
                self.egui = Some(EguiLayer::new(r.device(), r.overlay_format(), Some(&window)));
                self.renderer = Some(r);
            }
            Err(e) => {
                log::error!("no renderer: {e:#}");
                event_loop.exit();
                return;
            }
        }
        #[cfg(target_arch = "wasm32")]
        {
            let slot = self.pending.clone();
            let w = window.clone();
            wasm_bindgen_futures::spawn_local(async move {
                match Renderer::new_async(w, size.width.max(1), size.height.max(1)).await {
                    Ok(r) => *slot.borrow_mut() = Some(r),
                    Err(e) => log::error!("renderer: {e:#}"),
                }
            });
        }
        self.window = Some(window);
    }

    /// A browser's renderer arrived: drawn with from now on; the canvas as large as the page (at the screen's pixel
    /// density: winit makes the canvas's pixels the size asked times it, the touches and the pointer in them).
    fn arrived(&mut self) {
        // (asked again until it holds: a canvas not yet on the page takes no size)
        #[cfg(target_arch = "wasm32")]
        if let (Some(w), Some(page)) = (&self.window, web_sys::window()) {
            let size = [page.inner_width().ok().and_then(|v| v.as_f64()), page.inner_height().ok().and_then(|v| v.as_f64())];
            if let [Some(cw), Some(ch)] = size {
                let sf = w.scale_factor();
                let have = w.inner_size();
                // (the canvas's pixels: the page's, or the page's times the pixel ratio; either holds)
                let fits = |s: f64| (have.width as f64 - cw * s).abs() < 2.0 && (have.height as f64 - ch * s).abs() < 2.0;
                self.page_tries = if self.page != [cw, ch] { 0 } else { self.page_tries + 1 };
                if !fits(1.0) && !fits(sf) && self.page_tries.is_multiple_of(15) {
                    let _ = w.request_inner_size(winit::dpi::LogicalSize::new(cw, ch));
                }
                self.page = [cw, ch];
                // the events' positions are the page's pixels times the ratio; the screen draws in the canvas's
                if cw > 0.0 && have.width > 0 {
                    self.screen.set_input_scale(Some((have.width as f64 / (cw * sf)) as f32));
                }
            }
        }
        #[cfg(target_arch = "wasm32")]
        if let Some(mut r) = self.pending.borrow_mut().take() {
            if let Some(w) = &self.window {
                let s = w.inner_size();
                r.resize(s.width.max(1), s.height.max(1));
            }
            self.egui = Some(EguiLayer::new(r.device(), r.overlay_format(), self.window.as_deref()));
            self.renderer = Some(r);
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, event: WindowEvent) {
        if let (Some(e), Some(w)) = (&mut self.egui, &self.window) {
            e.on_event(w, &event);
        }
        self.screen.event(&event);
        match event {
            WindowEvent::Resized(s) => {
                if let Some(r) = self.renderer.as_mut() {
                    r.resize(s.width.max(1), s.height.max(1));
                }
            }
            // (during a job: it is cancelled first, the program ends when it has stopped)
            WindowEvent::CloseRequested if self.host.busy() => {
                self.host.cancel();
                self.quit_after = true;
            }
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::RedrawRequested => self.redraw(event_loop),
            _ => {}
        }
    }

    fn status(&mut self) -> Status {
        #[cfg(not(target_arch = "wasm32"))]
        let mut s = {
            self.host.follow();
            self.host.status(CAN)
        };
        #[cfg(target_arch = "wasm32")]
        let mut s = self.host.status();
        if let Some(m) = &self.message {
            s.message = Some(m.clone());
        }
        s
    }

    fn redraw(&mut self, event_loop: &ActiveEventLoop) {
        let status = self.status();
        if self.quit_after && !self.host.busy() {
            event_loop.exit();
            return;
        }
        let (Some(egui), Some(r)) = (self.egui.as_mut(), self.renderer.as_mut()) else { return };
        let screen = &mut self.screen;
        let mut commands = Vec::new();
        let mut frame = egui.run(self.window.as_deref(), r.size(), |ctx| {
            commands = screen.frame(ctx, &status);
            #[cfg(not(target_arch = "wasm32"))]
            for c in &commands {
                if let Command::Copy(text) = c {
                    ctx.copy_text(text.clone());
                }
            }
        });
        let cam = kumite_render::Camera::new(glam::Vec3::Z, glam::Vec3::ZERO, 1.0);
        if let Err(e) = r.render_with(&cam, &[], &[], &[], None, &[], Some(&mut frame)) {
            log::error!("render: {e:#}");
        }
        drop(frame);
        for c in commands {
            self.message = None;
            match self.host.command(c) {
                #[cfg(not(target_arch = "wasm32"))]
                Some(Command::Pick(part)) => {
                    if let Some(path) = choose(part) {
                        self.host.convert(part, path, None);
                    }
                }
                #[cfg(not(target_arch = "wasm32"))]
                Some(Command::Drop(part, paths)) => match dropped(part, &paths) {
                    Ok(path) => self.host.convert(part, path, None),
                    Err(e) => self.host.message = Some(e),
                },
                Some(Command::Play) if self.host.playable() && !self.host.busy() => self.play = true,
                Some(Command::Quit) if !self.host.busy() => event_loop.exit(),
                _ => {}
            }
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Jobs {
    fn playable(&self) -> bool {
        self.state.playable()
    }
}

/// Files dropped for a part: the image to convert among them (a sheet over the files it names, an archive, ...), or
/// the Half-Life folder (the folder dropped, or the one a dropped file is in).
#[cfg(not(target_arch = "wasm32"))]
fn dropped(part: Part, paths: &[PathBuf]) -> Result<PathBuf, String> {
    let first = paths.first().ok_or("Nothing was dropped.")?;
    if part == Part::Cs {
        return Ok(if first.is_dir() { first.clone() } else { first.parent().map_or_else(|| first.clone(), Path::to_path_buf) });
    }
    // (the arcade ROM set: its .zip, a folder of its files, or those files themselves: their folder)
    if part == Part::Arcade && !first.is_dir() {
        let zip = paths.len() == 1 && first.extension().is_some_and(|x| x.eq_ignore_ascii_case("zip"));
        return Ok(if zip { first.clone() } else { first.parent().map_or_else(|| first.clone(), Path::to_path_buf) });
    }
    if first.is_dir() {
        return crate::dataui::input_in(first, part).map_err(|e| format!("{e:#}"));
    }
    // (several files: the one to open among them, as the converters choose; the others beside it)
    let names: Vec<String> = paths.iter().filter_map(|p| p.file_name().map(|n| n.to_string_lossy().into_owned())).collect();
    struct Dropped<'a>(&'a [PathBuf], &'a [String]);
    impl ok_converter_common::disc::Folder for Dropped<'_> {
        fn files(&self) -> Vec<(String, u64)> {
            self.0.iter().zip(self.1).map(|(p, n)| (n.clone(), std::fs::metadata(p).map_or(0, |m| m.len()))).collect()
        }
        fn file(&self, _: &str) -> Option<Arc<dyn ok_converter_common::io::DiscFile>> {
            None
        }
    }
    let main = ok_converter_common::disc::main_image(&Dropped(paths, &names)).ok_or("That is no disc image (.cue, .bin, .chd, .iso, .ccd, .mds, .ecm, .pbp, or an archive of it).")?;
    let at = names.iter().position(|n| *n == main).unwrap_or(0);
    Ok(paths[at].clone())
}

/// The disc images the dialog offers (a sheet with the files it names beside it, or one file), both cases.
#[cfg(not(target_arch = "wasm32"))]
const IMAGES: [&str; 38] = [
    "cue", "bin", "iso", "chd", "ccd", "img", "mds", "mdf", "ecm", "pbp", "zip", "7z", "tar", "gz", "tgz", "xz", "txz", "zst", "tzst", "CUE", "BIN", "ISO", "CHD", "CCD", "IMG", "MDS", "MDF", "ECM",
    "PBP", "ZIP", "7Z", "TAR", "GZ", "TGZ", "XZ", "TXZ", "ZST", "TZST",
];

/// The system's dialog for the part's input: a disc image, or the game folder.
#[cfg(not(target_arch = "wasm32"))]
fn choose(part: Part) -> Option<PathBuf> {
    let what = match part {
        Part::Cs => "the Half-Life folder",
        Part::Arcade => "the ROM set (MAME's tekken3.zip)",
        _ => "the disc image (.cue, .bin, .chd, .iso, ...; or a .zip, .7z of it)",
    };
    let d = rfd::FileDialog::new().set_title(format!("{}: {what}", part.name()));
    match part {
        Part::Cs => d.pick_folder(),
        Part::Arcade => d.add_filter("ROM set (.zip)", &["zip", "ZIP"]).pick_file(),
        _ => d.add_filter("Disc image (.cue, .bin, .iso, .chd, .ccd, .img, .mds, .mdf, .ecm, .pbp; .zip, .7z, .tar, .gz, .xz, .zst)", &IMAGES).pick_file(),
    }
}

/// The title screen's Game data button (top right); pressed. (A browser's is the page's: egui has no input there.)
#[cfg(not(target_arch = "wasm32"))]
pub fn game_data_button(ctx: &egui::Context) -> bool {
    let area = egui::Area::new(egui::Id::unique("game_data_button")).anchor(egui::Align2::RIGHT_TOP, [-10.0, 10.0]);
    area.show(ctx, |ui| ui.button("Game data").clicked()).inner
}
