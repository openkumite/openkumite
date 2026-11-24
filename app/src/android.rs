//! Android (packaging/android): the game in its activity (OpenKumiteActivity.java, a NativeActivity), and the Game
//! data screen before it. The game data is the player's own, never in the app: for each game they choose the folder
//! that holds its disc image (or the Half-Life folder, or a .zip of either) with the system's picker, from Downloads,
//! an SD card or a USB drive; the activity copies what is needed into the app's files folder (assets.part), the game
//! converts it there (packs.rs: into `/sdcard/Android/data/io.github.openkumite/files/assets`, where `adb push` puts a
//! converted folder too) and removes the copy. A folder kumite_converter wrote is still taken as it is. The save is in
//! the app's internal folder; the log in logcat (tag `openkumite`).

use crate::dataui::Jobs;
use crate::debug::EguiLayer;
use crate::gamedata::{Abilities, Command, Screen};
use crate::packs::Part;
use crate::App;
use jni::objects::{JObject, JString};
use jni::{jni_sig, jni_str, JValue};
use kumite_render::Renderer;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, KeyEvent, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{NativeKeyCode, PhysicalKey};
use winit::platform::android::activity::{AndroidApp, WindowManagerFlags};
use winit::platform::android::EventLoopBuilderExtAndroid;
use winit::window::{Window, WindowId};

/// Android's back (the gesture or the button).
pub const BACK: PhysicalKey = PhysicalKey::Unidentified(NativeKeyCode::Android(4));

/// The activity's entry.
#[allow(unsafe_code)] // (no_mangle: the name android-activity calls)
#[no_mangle]
fn android_main(android: AndroidApp) {
    android_logger::init_once(android_logger::Config::default().with_tag("openkumite").with_max_level(log::LevelFilter::Info));
    // (a panic the game does not catch would otherwise say nothing)
    std::panic::set_hook(Box::new(|p| log::error!("{p}")));
    android.set_window_flags(WindowManagerFlags::KEEP_SCREEN_ON | WindowManagerFlags::FULLSCREEN, WindowManagerFlags::empty());
    if let Some(dir) = android.internal_data_path() {
        std::env::set_var("OPENKUMITE_SAVE_DIR", dir);
    }
    let files = android.external_data_path().unwrap_or_default();
    // (a copy cut off when the process ended: nothing copies it now)
    let _ = std::fs::remove_dir_all(files.join("assets.part"));
    let _ = std::fs::remove_dir_all(files.join("import.part"));
    let mut root = Root { now: Now::Data(Box::new(Data::new(&files, None, None))), android: android.clone(), files };
    if crate::packs::activate(&root.files.join("assets")).ready() {
        match game(&root.files) {
            Ok(app) => root.now = Now::Game(app),
            Err(e) => root.data_message(format!("{e:#}")),
        }
    }
    match EventLoop::builder().with_android_app(android).build() {
        Ok(event_loop) => {
            if let Err(e) = event_loop.run_app(&mut root) {
                log::error!("{e}");
            }
        }
        Err(e) => log::error!("no event loop: {e}"),
    }
    // (Android keeps the process; android-activity cannot run android_main twice in one: launched again, it panics)
    std::process::exit(0);
}

/// The game on the assets kept in the app's files folder.
fn game(files: &Path) -> anyhow::Result<Box<App>> {
    crate::packs::activate(&files.join("assets"));
    let app = crate::new_app(crate::packs::game_root(&files.join("assets"), crate::packs::game()), vec!["openkumite".into()])?;
    app.map(Box::new).ok_or_else(|| anyhow::anyhow!("the game did not start"))
}

/// The buttons at the top centre: Game data (on the title and the main menu: `with_data`) and Touch controls (there,
/// and in a fight's pause).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TopButton {
    GameData,
    TouchControls,
}

/// What a frame of the on-screen pad's UI gave: the pad as drawn (the editor's panel changed it), the top button and
/// the panel's button pressed, the chosen control's size set; the screen's size and pixels to a dp.
pub struct TouchFrame {
    pub touch: Option<crate::touchpad::TouchPad>,
    pub top: Option<TopButton>,
    pub panel: Option<crate::touchpad::PanelAction>,
    pub chosen_size: Option<f32>,
    pub size: [f32; 2],
    pub dp: f32,
}

/// A frame of the pad's UI applied: the panel's changes (Reset and Done saved: per device), the editor opened, the
/// Game data screen asked for.
pub fn touch_ui(pads: Option<&mut crate::gamepad::Gamepads>, game_data: &mut bool, debug: &mut crate::debug::Debug, f: TouchFrame) {
    use crate::touchpad::{Edit, PanelAction, Settings};
    let Some(g) = pads else { return };
    if let Some(t) = f.touch.filter(|t| t.edit.is_some()) {
        g.touch.settings = t.settings;
    }
    if let Some(s) = f.chosen_size {
        g.touch.size_chosen(s, f.size[0], f.size[1], f.dp);
    }
    match f.panel {
        Some(PanelAction::Reset) => {
            g.touch.settings = Settings { stick: g.touch.settings.stick, ..Settings::default() };
            save_touch(g, debug);
        }
        Some(PanelAction::Done) => close_touch_editor(g, debug),
        None => {}
    }
    match f.top {
        Some(TopButton::GameData) => *game_data = true,
        Some(TopButton::TouchControls) => {
            g.lift_fingers();
            g.touch.edit = Some(Edit::default());
        }
        None => {}
    }
}

/// The touch controls' editor closed, its layout saved.
pub fn close_touch_editor(g: &mut crate::gamepad::Gamepads, debug: &mut crate::debug::Debug) {
    g.touch.edit = None;
    save_touch(g, debug);
}

fn save_touch(g: &crate::gamepad::Gamepads, debug: &mut crate::debug::Debug) {
    if let Err(e) = g.save_touch() {
        debug.say(format!("the touch controls could not be saved: {e:#}"));
    }
}

/// The top buttons drawn; the one pressed.
pub fn top_buttons(ctx: &egui::Context, with_data: bool) -> Option<TopButton> {
    let area = egui::Area::new(egui::Id::unique("game_data_button")).anchor(egui::Align2::CENTER_TOP, [0.0, 8.0]);
    area.show(ctx, |ui| {
        ui.horizontal(|ui| {
            let data = with_data && ui.button("Game data").clicked();
            let touch = ui.button("Touch controls").clicked();
            if data {
                Some(TopButton::GameData)
            } else {
                touch.then_some(TopButton::TouchControls)
            }
        })
        .inner
    })
    .inner
}

struct Root {
    android: AndroidApp,
    /// The app's external files folder: assets/ (and assets.part/ while a copy goes).
    files: PathBuf,
    now: Now,
}

enum Now {
    Data(Box<Data>),
    Game(Box<App>),
}

impl Root {
    fn data_message(&mut self, m: String) {
        if let Now::Data(d) = &mut self.now {
            d.set_message(m);
        }
    }

    /// From one screen to the other: the game asked for the Game data screen; the data copied, or the player back.
    fn switch(&mut self, event_loop: &ActiveEventLoop) {
        match &mut self.now {
            Now::Game(app) if app.game_data => {
                let suspended = app.suspended;
                let mut d = Data::new(&self.files, app.window.take(), app.renderer.take());
                d.suspended = suspended;
                self.now = Now::Data(Box::new(d));
            }
            Now::Data(d) if d.play && !d.suspended => {
                d.play = false;
                match game(&self.files) {
                    Ok(mut app) => {
                        if let (Some(w), Some(r)) = (d.window.take(), d.renderer.take()) {
                            app.start(event_loop, w, r);
                        }
                        self.now = Now::Game(app);
                    }
                    Err(e) => {
                        log::error!("{e:#}");
                        d.set_message(format!("{e:#}"));
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

    fn suspended(&mut self, event_loop: &ActiveEventLoop) {
        match &mut self.now {
            Now::Game(app) => app.suspended(event_loop),
            Now::Data(d) => {
                d.suspended = true;
                if let Some(r) = d.renderer.as_mut() {
                    let _ = r.set_surface(None, 0, 0);
                }
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        match &mut self.now {
            Now::Game(app) => app.window_event(event_loop, id, event),
            Now::Data(d) => d.window_event(event_loop, &self.android, &self.files, event),
        }
        self.switch(event_loop);
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        match &mut self.now {
            Now::Game(app) => app.about_to_wait(event_loop),
            Now::Data(d) if d.suspended => event_loop.set_control_flow(ControlFlow::Wait),
            Now::Data(d) => {
                event_loop.set_control_flow(ControlFlow::Poll);
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

/// The Game data screen (crate::gamedata): each game's state, the pickers and the copy (the activity's), the job;
/// back to the game.
struct Data {
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
    egui: Option<EguiLayer>,
    suspended: bool,
    jobs: Jobs,
    screen: Screen,
    /// The part whose files are being picked or copied.
    picking: Option<Part>,
    /// Start the game (the data converted, or the player back to it).
    play: bool,
}

/// The copy as the activity tells it.
struct Import {
    /// 0 none, 1 copying, 2 done, 3 failed.
    state: i32,
    done: i64,
    total: i64,
    error: Option<String>,
}

/// What the activity's picker picks (OpenKumiteActivity.java's).
const PICK_ZIP: i32 = 1;
const PICK_DISC: i32 = 2;
const PICK_GAME: i32 = 3;

/// What Android can do on the Game data screen: folders picked (the system's picker), a .zip imported.
const CAN: Abilities = Abilities { quit: false, folders: true, copy: false, drop: false, import: Some("IMPORT A .ZIP") };

impl Data {
    fn new(files: &Path, window: Option<Arc<Window>>, renderer: Option<Renderer>) -> Self {
        let egui = renderer.as_ref().map(|r| EguiLayer::new(r.device(), r.overlay_format(), window.as_deref()));
        Self { window, renderer, egui, suspended: false, jobs: Jobs::new(&files.join("assets")), screen: Screen::new(), picking: None, play: false }
    }

    fn set_message(&mut self, m: String) {
        self.jobs.message = Some(m);
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if let (Some(w), Some(r)) = (self.window.clone(), self.renderer.as_mut()) {
            if std::mem::take(&mut self.suspended) {
                let size = w.inner_size();
                if let Err(e) = r.set_surface(Some(w.into()), size.width, size.height) {
                    log::error!("the window's surface could not be made again: {e:#}");
                    event_loop.exit();
                }
            }
            return;
        }
        let window = match event_loop.create_window(Window::default_attributes().with_title("OpenKumite")) {
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

    fn window_event(&mut self, event_loop: &ActiveEventLoop, android: &AndroidApp, files: &Path, event: WindowEvent) {
        if let (Some(e), Some(w)) = (&mut self.egui, &self.window) {
            e.on_event(w, &event);
        }
        // back on the games' list: to the game, else out (not while copying or converting); elsewhere the screen's
        if let WindowEvent::KeyboardInput { event: KeyEvent { physical_key: BACK, state: ElementState::Pressed, repeat: false, .. }, .. } = &event {
            let copying = import(android).is_some_and(|i| i.state == 1);
            if self.screen.at_home() && !copying && !self.jobs.busy() && self.jobs.pending.is_none() && self.jobs.outcome.is_none() {
                if self.jobs.state.playable() {
                    self.play = true;
                } else {
                    event_loop.exit();
                }
                return;
            }
        }
        self.screen.event(&event);
        match event {
            WindowEvent::Resized(s) => {
                if let Some(r) = self.renderer.as_mut() {
                    r.resize(s.width, s.height);
                }
            }
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::RedrawRequested if !self.suspended => self.redraw(android, files),
            _ => {}
        }
    }

    /// A copy finished: a folder kumite_converter wrote kept as it is; else its files converted (the copy removed).
    fn copied(&mut self, files: &Path) {
        let part = self.picking.take();
        let copy = files.join("assets.part");
        if crate::assetdir::conversion_in(&copy).is_ok() {
            match keep(files) {
                Ok(()) => {
                    log::info!("a converted game copied and checked");
                    self.jobs = Jobs::new(&files.join("assets"));
                    self.set_message("The converted game is kept.".into());
                }
                Err(e) => self.set_message(format!("{e:#}")),
            }
            return;
        }
        let Some(part) = part else {
            let _ = std::fs::remove_dir_all(&copy);
            return;
        };
        // (the copy renamed: a new pick does not write over it while it converts)
        let scratch = files.join("import.part");
        let _ = std::fs::remove_dir_all(&scratch);
        if let Err(e) = std::fs::rename(&copy, &scratch) {
            let _ = std::fs::remove_dir_all(&copy);
            self.set_message(format!("{e}"));
            return;
        }
        match crate::dataui::input_in(&scratch, part) {
            Ok(input) => self.jobs.convert(part, input, Some(scratch)),
            Err(e) => {
                let _ = std::fs::remove_dir_all(&scratch);
                self.set_message(format!("{e:#}"));
            }
        }
    }

    /// The copy and the job followed, and the screen drawn.
    fn redraw(&mut self, android: &AndroidApp, files: &Path) {
        self.jobs.follow();
        let now = import(android);
        match &now {
            Some(i) if i.state == 2 => {
                java(android, |env, a| env.call_method(a, jni_str!("importSeen"), jni_sig!("()V"), &[]).map(|_| ()));
                self.copied(files);
            }
            Some(i) if i.state == 3 => {
                java(android, |env, a| env.call_method(a, jni_str!("importSeen"), jni_sig!("()V"), &[]).map(|_| ()));
                self.picking = None;
                self.jobs.message = i.error.clone();
            }
            _ => {}
        }
        let mut status = self.jobs.status(CAN);
        status.picking = self.picking;
        status.copy = now.filter(|i| i.state == 1).map(|i| (i.done.max(0) as u64, i.total.max(0) as u64));
        let (Some(egui), Some(r)) = (self.egui.as_mut(), self.renderer.as_mut()) else { return };
        let screen = &mut self.screen;
        let mut commands = Vec::new();
        let mut frame = egui.run(self.window.as_deref(), r.size(), |ctx| commands = screen.frame(ctx, &status));
        let cam = kumite_render::Camera::new(glam::Vec3::Z, glam::Vec3::ZERO, 1.0);
        if let Err(e) = r.render_with(&cam, &[], &[], &[], None, &[], Some(&mut frame)) {
            log::error!("render: {e:#}");
        }
        drop(frame);
        for c in commands {
            let pick = match self.jobs.command(c) {
                Some(Command::Pick(part)) => {
                    self.picking = Some(part);
                    Some(if part == Part::Cs { PICK_GAME } else { PICK_DISC })
                }
                // (a .zip: for the first game not converted yet, or Tekken 3)
                Some(Command::Import) => {
                    self.picking = Some([Part::T3, Part::T2, Part::Cs].into_iter().find(|p| self.jobs.state.get(*p).is_none()).unwrap_or(Part::T3));
                    Some(PICK_ZIP)
                }
                Some(Command::Play) if self.jobs.state.playable() && !self.jobs.busy() => {
                    self.play = true;
                    None
                }
                _ => None,
            };
            if let Some(what) = pick {
                self.jobs.message = None;
                java(android, |env, a| env.call_method(a, jni_str!("pick"), jni_sig!("(I)V"), &[JValue::Int(what)]).map(|_| ()));
            }
        }
    }
}

/// The copy in assets.part, a converted game (its manifest.json and format checked, as the desktop's), made the Tekken
/// 3 pack kept, over the one before (its packs kept: installed again over it); else removed, and why.
fn keep(files: &Path) -> anyhow::Result<()> {
    let part = files.join("assets.part");
    let kept = (|| {
        let found = crate::assetdir::conversion_in(&part)?;
        let assets = files.join("assets");
        let packs = assets.join(crate::packs::PACKS);
        if packs.is_dir() {
            std::fs::rename(&packs, found.join(crate::packs::PACKS))?;
        }
        if assets.exists() {
            std::fs::remove_dir_all(&assets)?;
        }
        std::fs::rename(&found, &assets)?;
        anyhow::Ok(())
    })();
    let _ = std::fs::remove_dir_all(&part);
    kept
}

/// The copy as the activity tells it (None: it could not be asked).
fn import(android: &AndroidApp) -> Option<Import> {
    java(android, |env, a| {
        let error = env.get_field(a, jni_str!("importError"), jni_sig!("Ljava/lang/String;"))?.l()?;
        let error = if error.is_null() { None } else { Some(env.cast_local::<JString>(error)?.try_to_string(env)?) };
        let long = |env: &mut jni::Env, name| env.get_field(a, name, jni_sig!("J"))?.j();
        Ok(Import { state: env.get_field(a, jni_str!("importState"), jni_sig!("I"))?.i()?, done: long(env, jni_str!("importDone"))?, total: long(env, jni_str!("importTotal"))?, error })
    })
}

/// Run `f` on the activity (OpenKumiteActivity.java) from this thread (attached to the VM for it); a JNI failure, or
/// a Java exception it raised, logged: None.
#[allow(unsafe_code)] // JNI: the VM and the activity android-activity holds
fn java<T>(android: &AndroidApp, f: impl FnOnce(&mut jni::Env, &JObject) -> jni::errors::Result<T>) -> Option<T> {
    // SAFETY: android-activity's pointers to the process's Java VM and to its global reference of the activity, both
    // valid while the activity runs (this event loop)
    let vm = unsafe { jni::JavaVM::from_raw(android.vm_as_ptr().cast()) };
    let out = vm.attach_current_thread(|env| {
        let activity = unsafe { JObject::from_raw(env, android.activity_as_ptr().cast()) };
        f(env, &activity)
    });
    out.map_err(|e| log::error!("JNI: {e}")).ok()
}
