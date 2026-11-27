//! The Game data screen, one for every platform (the desktops' datascreen.rs, Android's android.rs, the browser's
//! web.rs host it): where the player converts their own copies, drawn as the game's menus are (black, the title over
//! a list, the chosen row on the menu's shaded bar) before there is any game art to draw them with.
//!
//! The screen knows nothing of the platform: each frame its host gives the game data's [`Status`] (what is
//! converted, the job running and its log, a picked image waiting for the space check, the last job's outcome) and
//! does the [`Command`]s the player gave (pick an image, convert, cancel, remove, play). Its input comes from the
//! window's events as they are on every platform (keys, the mouse and a touchpad, touches, files dropped) and from the
//! gamepads (`crate::gamepad`), not through egui (the browser build feeds egui no input), and egui only paints.
//!
//! Pages: the games (HOME), a game's own (choose its image, remove it), the removal's question, the space check, the
//! job (its progress, time left, cancel), its outcome, the log. Back goes up; the job's pages come by themselves.

pub mod progress;
#[cfg(target_arch = "wasm32")]
pub mod web;

use crate::controls::{Controls, PadButton};
#[cfg(not(target_arch = "wasm32"))]
pub use crate::packs::Part;
use egui::{pos2, vec2, Align2, Color32, FontFamily, FontId, Pos2, Rect, Shape, Stroke};
use progress::{clock, Kind, Progress};
use std::path::PathBuf;
#[cfg(target_arch = "wasm32")]
pub use web::Part;
use web_time::Instant;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, TouchPhase, WindowEvent};
use winit::keyboard::{KeyCode, NativeKeyCode, PhysicalKey};

/// A part as the screen shows it.
#[derive(Clone, Debug, Default)]
pub struct PartView {
    pub converted: bool,
    /// Bytes on the device.
    pub size: u64,
    /// What its copy did not hold (`kumite_packs::missing_note`).
    pub note: Option<String>,
    /// Converted by an older version (`kumite_packs::Part::outdated`): it plays, without what the new one adds.
    pub outdated: bool,
    /// What its pack left out (its manifest's "skipped": [`optional`]'s names).
    pub skipped: Vec<String>,
    /// What its next conversion leaves out, as the player chose it (None: what its pack left out).
    pub skip: Option<Vec<String>>,
}

/// A job running.
#[derive(Clone, Debug)]
pub struct JobView {
    pub kind: Kind,
    /// Seconds since it started.
    pub seconds: f64,
    /// Its log so far.
    pub log: Vec<String>,
    pub cancellable: bool,
    /// Cancel asked: it stops at its next write.
    pub cancelling: bool,
}

/// A job finished (until the player has seen it).
#[derive(Clone, Debug)]
pub struct Outcome {
    pub kind: Kind,
    pub ok: bool,
    pub cancelled: bool,
    pub message: String,
    pub log: Vec<String>,
}

/// An image picked, waiting for the player: the device may have too little space for its conversion.
#[derive(Clone, Debug)]
pub struct Pending {
    pub part: Part,
    pub image: String,
    pub needed: u64,
    pub free: u64,
}

/// What the platform can do.
#[derive(Clone, Copy, Debug, Default)]
pub struct Abilities {
    /// A Quit row (the desktops; a phone's back leaves, a browser's tab closes).
    pub quit: bool,
    /// A game's input is chosen as a folder (Android's picker; and Counter-Strike's everywhere).
    pub folders: bool,
    /// The log can be copied.
    pub copy: bool,
    /// Files can be dropped on the window.
    pub drop: bool,
    /// An extra row, platform's own (Android: a .zip; the browser: a folder kumite_converter wrote).
    pub import: Option<&'static str>,
}

/// The game data as the host knows it.
#[derive(Clone, Debug, Default)]
pub struct Status {
    /// Tekken 3, Tekken 2, Counter-Strike, the arcade stages, Tekken Tag Tournament (`Part::ALL`'s order).
    pub parts: [PartView; 5],
    pub playable: bool,
    /// The main game (Tekken 3 or Tekken Tag Tournament).
    pub main: Part,
    /// Not known yet (the browser asks its worker).
    pub loading: bool,
    pub job: Option<JobView>,
    pub outcome: Option<Outcome>,
    pub pending: Option<Pending>,
    /// The system's picker is open (or its files are being copied: Android), for this part.
    pub picking: Option<Part>,
    /// The picked files being copied: bytes done, all (Android).
    pub copy: Option<(u64, u64)>,
    /// Files dropped on the page, waiting for a part (the browser).
    pub dropped: bool,
    /// Files dragged over the page (the browser; natively the window says so itself).
    pub hovering: bool,
    /// A message for the player (a pick that failed, ...), until the next command.
    pub message: Option<String>,
    pub can: Abilities,
}

/// What the player asked for.
#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    /// Open the system's picker for the part's image (or folder).
    Pick(Part),
    /// Files dropped for the part (paths natively; the browser's are in its page).
    Drop(Part, Vec<PathBuf>),
    /// Convert the pending image though space looks short.
    ConvertAnyway,
    /// Forget the pending image.
    DropPending,
    Remove(Part),
    Cancel,
    /// What the part's next conversion leaves out ([`optional`]'s names).
    Skip(Part, Vec<String>),
    /// Remove that optional part from its converted pack.
    Strip(Part, String),
    /// The outcome seen.
    Seen,
    Play,
    Quit,
    Import,
    /// Copy this text (the log) where the platform can.
    Copy(String),
    /// Make this game the main one (then start it, when it can).
    MainGame(Part),
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Page {
    Home,
    Game(Part),
    /// The removal's question: of the part, or of its optional part (an index of [`optional`]).
    Remove(Part, Option<usize>),
}

/// A row's action.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Act {
    Part(Part),
    Pick(Part),
    AskRemove(Part, Option<usize>),
    Remove(Part, Option<usize>),
    /// Keep or leave out an optional part ([`optional`]'s index).
    Skip(Part, usize),
    Back,
    Play,
    Quit,
    Import,
    Cancel,
    Details,
    Copy,
    ConvertAnyway,
    DropPending,
    Seen,
    MainGame,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Device {
    Keys,
    Pointer,
    Touch,
    Pad,
}

/// A row: its text, whether it can be chosen, its action, and a note on its right.
struct Row {
    text: String,
    on: bool,
    act: Act,
    right: Option<(String, Color32)>,
}

const GOLD: Color32 = Color32::from_rgb(242, 193, 78);
const WHITE: Color32 = Color32::from_rgb(245, 240, 230);
const GREY: Color32 = Color32::from_rgb(150, 146, 140);
const DIM: Color32 = Color32::from_rgb(80, 78, 74);
const GREEN: Color32 = Color32::from_rgb(127, 216, 143);
const ORANGE: Color32 = Color32::from_rgb(255, 179, 107);
const RED: Color32 = Color32::from_rgb(255, 107, 107);

pub fn part_title(p: Part) -> &'static str {
    match p {
        Part::T3 => "TEKKEN 3",
        Part::T2 => "TEKKEN 2",
        Part::Cs => "COUNTER-STRIKE",
        Part::Arcade => "TEKKEN 3 ARCADE",
        Part::Ttt => "TEKKEN TAG TOURNAMENT",
    }
}

fn part_about(p: Part) -> &'static str {
    match p {
        Part::T3 => "PlayStation, USA (SLUS-00402). The game itself: needed.",
        Part::T2 => "PlayStation, USA v1.1. Its fighters join the roster, with their stages, music and endings. Optional.",
        Part::Cs => "Half-Life with Counter-Strike. Its levels become arenas, and two more Tekken Force stages. Optional.",
        Part::Arcade => "The arcade game (Namco System 12). Its 3D stages replace the PlayStation game's. Optional.",
        Part::Ttt => "PlayStation 2, USA v2.00 (SLUS-20001). A game of its own: needed only as the main game.",
    }
}

fn part_input(p: Part, folders: bool) -> &'static str {
    match (p, folders) {
        (Part::Cs, _) => "Choose the Half-Life folder (with cstrike/ and valve/).",
        (Part::Arcade, true) => "Choose the folder that holds its ROM set, MAME's tekken3.zip: a folder of its own, not Download itself.",
        (Part::Arcade, false) => "Choose its ROM set: MAME's tekken3.zip.",
        (Part::Ttt, false) => "Choose its disc image: the .cue (its .bin file beside it), .iso, .chd, or a .zip of it.",
        (_, true) => "Choose the folder that holds its disc image (a folder of its own, not Download itself): .cue with its .bin files, .chd, .iso, .pbp, or a .zip or .7z of them.",
        _ => "Choose its disc image: the .cue (its .bin files beside it), .chd, .iso, .pbp, .ccd, .mds, .ecm, or a .zip, .7z, .tar, .gz, .xz or .zst of it.",
    }
}

/// What a part's copy did not hold (its manifest's `source.missing`: an ISO holds no movies or XA music, an image
/// without its track 2 no CD audio), as the screen says it; None when it held everything. The game plays without them.
pub fn missing_note(manifest: &serde_json::Value) -> Option<String> {
    let missing: Vec<&str> = manifest["source"]["missing"].as_array()?.iter().filter_map(serde_json::Value::as_str).collect();
    let (last, rest) = missing.split_last()?;
    let what = if rest.is_empty() { last.to_string() } else { format!("{} and {last}", rest.join(", ")) };
    Some(format!("Without {what}: the disc image did not hold them. Convert again from a full image (.cue with its .bin files, .chd, ...) to add them."))
}

/// What a part's conversion can leave out, the game playing without it: the converters' names (`skip`, the
/// manifest's "skipped"), as the screen says them, and about how many bytes each is (the converters' `OPTIONAL`, which
/// the browser's game does not link; a test checks them).
pub fn optional(p: Part) -> &'static [(&'static str, &'static str, u64)] {
    match p {
        Part::T3 => &[("endings", "ENDING MOVIES", 250_000_000), ("opening", "OPENING MOVIE", 34_000_000), ("movies", "PROLOGUE MOVIE", 7_000_000), ("music", "MUSIC", 731_000_000)],
        Part::T2 => &[("endings", "ENDING MOVIES", 267_000_000), ("music", "MUSIC", 668_000_000)],
        Part::Ttt => &[("endings", "ENDINGS", 178_000_000), ("opening", "OPENING MOVIE", 89_000_000), ("movies", "ARCADE MOVIE", 30_000_000), ("music", "MUSIC", 209_000_000)],
        Part::Cs | Part::Arcade => &[],
    }
}

/// What a pack left out (its manifest's "skipped").
pub fn skipped(manifest: &serde_json::Value) -> Vec<String> {
    manifest["skipped"].as_array().into_iter().flatten().filter_map(|s| s.as_str().map(str::to_string)).collect()
}

/// A part converted by an older version, as the screen says it.
pub const OUTDATED: &str = "Converted by an older OpenKumite: it plays as before, without what this version adds until it is converted again (CONVERT AGAIN).";

/// Tekken Tag Tournament the main game, converted, but not playable (a pack without its assets/): what the game says.
pub const NOT_YET: &str = "Tekken Tag Tournament is converted; it is not playable in this version yet";

pub fn gb(bytes: u64) -> String {
    format!("{:.1} GB", bytes as f64 / 1e9)
}

/// The fonts: the game's look (Big Shoulders Display, OFL: app/fonts), egui's own after them for what they lack.
pub fn install_fonts(ctx: &egui::Context) {
    let id = egui::Id::unique("ok_gamedata_fonts");
    if ctx.data(|d| d.get_temp::<bool>(id)).unwrap_or(false) {
        return;
    }
    let mut fonts = egui::FontDefinitions::default();
    let base = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    for (name, bytes) in [("ok-display", &include_bytes!("../../fonts/BigShouldersDisplay-ExtraBold.ttf")[..]), ("ok-text", &include_bytes!("../../fonts/BigShouldersDisplay-Medium.ttf")[..])] {
        fonts.font_data.insert(name.into(), std::sync::Arc::new(egui::FontData::from_static(bytes)));
        fonts.families.insert(FontFamily::Name(name.into()), std::iter::once(name.to_string()).chain(base.iter().cloned()).collect());
    }
    ctx.set_fonts(fonts);
    ctx.data_mut(|d| d.insert_temp(id, true));
}

fn display(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("ok-display".into()))
}

fn text(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("ok-text".into()))
}

pub struct Screen {
    page: Page,
    focus: usize,
    /// The pointer, in points, and whether it moved since the last frame (hovering moves the focus only then).
    pointer: Option<Pos2>,
    moved: bool,
    /// The row a press (mouse, touch) began on, and where.
    pressed: Option<(usize, Pos2)>,
    /// A touch dragging (the log scrolls): its id and last y.
    drag: Option<(u64, f32)>,
    /// The rows' rectangles as last drawn.
    hits: Vec<Rect>,
    /// The cards a drop can land on (the games' rows), as last drawn.
    drop_targets: Vec<(Rect, Part)>,
    /// The log page open (over the job or its outcome), and its scroll: lines up from the end.
    log: bool,
    scroll: f32,
    /// Window pixels per point (input positions come in pixels), and the host's own scale from input positions to the
    /// screen's points when it knows better (a browser: its canvas's pixels are not always the events' pixels).
    ppp: f32,
    input_scale: Option<f32>,
    device: Device,
    started: Instant,
    /// Files dragged over the window; dropped (natively).
    hovering: bool,
    drops: Vec<PathBuf>,
    /// The rows' actions last frame (another set: the focus on the first), and whether the player's own move
    /// (`go`) set the focus for the new set already.
    acts: Vec<Act>,
    went: bool,
    /// Navigation asked since the last frame.
    nav: Vec<Nav>,
    shift: bool,
    gamepads: Option<crate::gamepad::Gamepads>,
    controls: Controls,
    /// The job's progress, kept across frames (its log read once).
    progress: Option<(Kind, Progress, usize)>,
    /// "Copied", for a moment.
    toast: Option<(String, Instant)>,
    commands: Vec<Command>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Nav {
    Up,
    Down,
    Go,
    Back,
    PageUp,
    PageDown,
    Top,
    End,
}

impl Default for Screen {
    fn default() -> Self {
        Self::new()
    }
}

impl Screen {
    pub fn new() -> Self {
        Self {
            page: Page::Home,
            focus: 0,
            pointer: None,
            moved: false,
            pressed: None,
            drag: None,
            hits: Vec::new(),
            drop_targets: Vec::new(),
            log: false,
            scroll: 0.0,
            ppp: 1.0,
            input_scale: None,
            device: Device::Keys,
            started: Instant::now(),
            hovering: false,
            drops: Vec::new(),
            acts: Vec::new(),
            went: false,
            nav: Vec::new(),
            shift: false,
            gamepads: None,
            controls: Controls::default(),
            progress: None,
            toast: None,
            commands: Vec::new(),
        }
    }

    /// With the gamepads (not in tests: no devices there; Android's come as keys, `key_nav`).
    #[cfg_attr(target_os = "android", allow(dead_code))]
    pub fn with_gamepads(mut self) -> Self {
        self.gamepads = Some(crate::gamepad::Gamepads::new());
        self
    }

    /// A window event (in the window's pixels); true when the screen took it.
    pub fn event(&mut self, event: &WindowEvent) -> bool {
        match event {
            WindowEvent::CursorMoved { position, .. } => {
                let p = pos2(position.x as f32, position.y as f32) * self.input_scale();
                if self.pointer.is_none_or(|q| (q - p).length() > 0.5) {
                    self.moved = true;
                    if self.device != Device::Touch {
                        self.device = Device::Pointer;
                    }
                }
                self.pointer = Some(p);
            }
            WindowEvent::CursorLeft { .. } => self.pointer = None,
            WindowEvent::MouseInput { state, button: MouseButton::Left, .. } => {
                self.device = Device::Pointer;
                match state {
                    ElementState::Pressed => self.press(),
                    ElementState::Released => self.release(),
                }
            }
            WindowEvent::MouseInput { state: ElementState::Pressed, button: MouseButton::Back, .. } => self.nav.push(Nav::Back),
            WindowEvent::MouseWheel { delta, .. } => {
                let lines = match delta {
                    MouseScrollDelta::LineDelta(_, y) => *y,
                    MouseScrollDelta::PixelDelta(p) => p.y as f32 * self.input_scale() / 18.0,
                };
                self.scroll = (self.scroll + lines * 3.0).max(0.0);
            }
            WindowEvent::Touch(t) => {
                let p = pos2(t.location.x as f32, t.location.y as f32) * self.input_scale();
                self.device = Device::Touch;
                self.pointer = Some(p);
                match t.phase {
                    TouchPhase::Started => {
                        self.drag = Some((t.id, p.y));
                        self.press();
                    }
                    TouchPhase::Moved => {
                        if let Some((id, y)) = self.drag.filter(|d| d.0 == t.id) {
                            // (a finger that moves is scrolling, not tapping)
                            if self.pressed.is_some_and(|(_, at)| (at - p).length() > 12.0) {
                                self.pressed = None;
                            }
                            if self.log {
                                self.scroll = (self.scroll + (p.y - y) / 16.0).max(0.0);
                            }
                            self.drag = Some((id, p.y));
                        }
                    }
                    TouchPhase::Ended => {
                        self.release();
                        self.drag = None;
                        self.pointer = None;
                    }
                    TouchPhase::Cancelled => {
                        self.pressed = None;
                        self.drag = None;
                        self.pointer = None;
                    }
                }
            }
            WindowEvent::ModifiersChanged(m) => {
                self.shift = m.state().shift_key();
                return false;
            }
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => return self.key(&event.physical_key),
            WindowEvent::HoveredFile(_) => self.hovering = true,
            WindowEvent::HoveredFileCancelled => self.hovering = false,
            WindowEvent::DroppedFile(p) => {
                self.hovering = false;
                self.drops.push(p.clone());
            }
            _ => return false,
        }
        true
    }

    /// Input positions (the window's events') to the screen's points, when it is not egui's pixels per point.
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    pub fn set_input_scale(&mut self, s: Option<f32>) {
        self.input_scale = s;
    }

    fn input_scale(&self) -> f32 {
        self.input_scale.unwrap_or(1.0 / self.ppp.max(0.01))
    }

    /// On the games' list (the first page), no other over it: a phone's back leaves from here.
    #[cfg_attr(not(target_os = "android"), allow(dead_code))]
    pub fn at_home(&self) -> bool {
        self.page == Page::Home && !self.log
    }

    /// A key pressed (or repeating); true when the screen took it.
    pub fn key(&mut self, physical: &PhysicalKey) -> bool {
        let Some(mut n) = key_nav(physical) else { return false };
        if self.shift && *physical == PhysicalKey::Code(KeyCode::Tab) {
            n = Nav::Up;
        }
        self.device = Device::Keys;
        self.nav.push(n);
        true
    }

    fn hit(&self) -> Option<usize> {
        let p = self.pointer?;
        self.hits.iter().position(|r| r.contains(p))
    }

    fn press(&mut self) {
        self.pressed = self.hit().map(|k| (k, self.pointer.unwrap_or_default()));
        if let Some((k, _)) = self.pressed {
            self.focus = k;
        }
    }

    fn release(&mut self) {
        if let (Some((k, _)), Some(h)) = (self.pressed.take(), self.hit()) {
            if k == h {
                self.focus = k;
                self.nav.push(Nav::Go);
            }
        }
    }

    /// The game data as it is now drawn, and the player's input acted on; what they asked for.
    pub fn frame(&mut self, ctx: &egui::Context, status: &Status) -> Vec<Command> {
        install_fonts(ctx);
        self.ppp = ctx.pixels_per_point();
        self.pads();
        self.follow(status);
        let rows = self.rows(status);
        // another set of rows (the job's page came, its outcome): the focus on the first
        let acts: Vec<Act> = rows.iter().map(|r| r.act).collect();
        if acts != self.acts {
            if !self.went {
                self.focus = 0;
            }
            self.acts = acts;
        }
        self.went = false;
        // focus: on a row that can be chosen
        if rows.is_empty() {
            self.focus = 0;
        } else {
            self.focus = self.focus.min(rows.len() - 1);
            if !rows[self.focus].on {
                self.focus = rows.iter().position(|r| r.on).unwrap_or(0);
            }
        }
        // the pointer over a row chooses it (as it moves)
        if std::mem::take(&mut self.moved) && self.device == Device::Pointer {
            if let Some(k) = self.hit().filter(|&k| rows.get(k).is_some_and(|r| r.on)) {
                self.focus = k;
            }
        }
        // (each key on the rows as the one before left them: a page it opened has its own)
        for n in std::mem::take(&mut self.nav) {
            let rows = self.rows(status);
            self.navigate(n, &rows, status);
        }
        // dropped files: for the game under the pointer, else the one chosen, else the first not converted
        if !self.drops.is_empty() || status.dropped {
            let at = self.pointer.and_then(|p| self.drop_targets.iter().find(|(r, _)| r.contains(p)).map(|t| t.1));
            let page = match self.page {
                Page::Game(p) | Page::Remove(p, _) => Some(p),
                Page::Home => rows.get(self.focus).and_then(|r| if let Act::Part(p) = r.act { Some(p) } else { None }),
            };
            // (a disc image's: not the arcade ROM set's part)
            let first = [Part::T3, Part::T2, Part::Cs].into_iter().find(|p| !status.parts[part_index(*p)].converted).unwrap_or(Part::T3);
            let part = at.or(page).unwrap_or(first);
            if status.job.is_none() {
                self.commands.push(Command::Drop(part, std::mem::take(&mut self.drops)));
            } else {
                self.drops.clear();
            }
        }
        let rows = self.rows(status);
        self.draw(ctx, status, &rows);
        std::mem::take(&mut self.commands)
    }

    /// The job's pages come and go by themselves; the log closes with them.
    fn follow(&mut self, status: &Status) {
        match &status.job {
            Some(j) => {
                let fresh = self.progress.as_ref().is_none_or(|(k, _, _)| *k != j.kind);
                if fresh {
                    self.progress = Some((j.kind, Progress::new(j.kind), 0));
                }
                if let Some((_, p, seen)) = self.progress.as_mut() {
                    // (a line's time: now, as near as the host tells)
                    for line in j.log.iter().skip(*seen) {
                        p.line(line, j.seconds);
                    }
                    *seen = j.log.len();
                }
            }
            None => {
                if status.outcome.is_none() {
                    self.log = false;
                }
                if status.outcome.is_none() {
                    self.progress = None;
                }
            }
        }
    }

    fn pads(&mut self) {
        let Some(g) = self.gamepads.as_mut() else { return };
        for (_, b, _) in g.poll(&self.controls) {
            self.device = Device::Pad;
            self.nav.push(match b {
                PadButton::Up => Nav::Up,
                PadButton::Down => Nav::Down,
                PadButton::Left => Nav::Up,
                PadButton::Right => Nav::Down,
                PadButton::South | PadButton::Start => Nav::Go,
                PadButton::East | PadButton::Select | PadButton::Home => Nav::Back,
                PadButton::L1 | PadButton::L2 => Nav::PageUp,
                PadButton::R1 | PadButton::R2 => Nav::PageDown,
                _ => continue,
            });
        }
    }

    fn navigate(&mut self, n: Nav, rows: &[Row], status: &Status) {
        let enabled: Vec<usize> = rows.iter().enumerate().filter(|(_, r)| r.on).map(|(k, _)| k).collect();
        let at = enabled.iter().position(|&k| k == self.focus);
        match n {
            Nav::Up | Nav::Down if enabled.is_empty() => {}
            Nav::Up => self.focus = enabled[at.map_or(0, |a| (a + enabled.len() - 1) % enabled.len())],
            Nav::Down => self.focus = enabled[at.map_or(0, |a| (a + 1) % enabled.len())],
            Nav::Go => {
                if let Some(r) = rows.get(self.focus).filter(|r| r.on) {
                    self.act(r.act, status);
                }
            }
            Nav::Back => {
                if self.log {
                    self.log = false;
                } else if status.pending.is_some() {
                    self.commands.push(Command::DropPending);
                } else if status.outcome.is_some() && status.job.is_none() {
                    self.commands.push(Command::Seen);
                } else if status.job.is_none() && status.copy.is_none() {
                    match self.page {
                        Page::Home => {}
                        Page::Game(p) => self.go(Page::Home, Some(Act::Part(p)), status),
                        Page::Remove(p, _) => self.go(Page::Game(p), None, status),
                    }
                }
            }
            Nav::PageUp => self.scroll += 12.0,
            Nav::PageDown => self.scroll = (self.scroll - 12.0).max(0.0),
            Nav::Top => self.scroll = f32::MAX,
            Nav::End => self.scroll = 0.0,
        }
    }

    /// To another page, the focus on the row with `on` (else its first).
    fn go(&mut self, page: Page, on: Option<Act>, status: &Status) {
        self.page = page;
        self.focus = 0;
        self.went = true;
        if let Some(a) = on {
            self.focus = self.rows(status).iter().position(|r| r.act == a).unwrap_or(0);
        }
    }

    fn act(&mut self, a: Act, status: &Status) {
        match a {
            Act::Part(p) => self.go(Page::Game(p), None, status),
            Act::Pick(p) => self.commands.push(Command::Pick(p)),
            Act::AskRemove(p, k) => self.go(Page::Remove(p, k), Some(Act::Back), status),
            Act::Remove(p, k) => {
                self.commands.push(match k.and_then(|k| optional(p).get(k)) {
                    Some(o) => Command::Strip(p, o.0.into()),
                    None => Command::Remove(p),
                });
                self.go(Page::Game(p), None, status);
            }
            Act::Back => match self.page {
                Page::Remove(p, _) => self.go(Page::Game(p), None, status),
                Page::Game(p) => self.go(Page::Home, Some(Act::Part(p)), status),
                Page::Home => {}
            },
            Act::Play => self.commands.push(Command::Play),
            Act::Quit => self.commands.push(Command::Quit),
            Act::Import => self.commands.push(Command::Import),
            Act::Cancel => self.commands.push(Command::Cancel),
            Act::Details => {
                self.log = !self.log;
                self.scroll = 0.0;
            }
            Act::Copy => {
                let log = status.job.as_ref().map(|j| &j.log).or(status.outcome.as_ref().map(|o| &o.log));
                let text = log.map(|l| l.join("\n")).unwrap_or_default();
                self.toast = Some(("COPIED".into(), Instant::now()));
                self.commands.push(Command::Copy(text));
            }
            Act::ConvertAnyway => self.commands.push(Command::ConvertAnyway),
            Act::DropPending => self.commands.push(Command::DropPending),
            Act::Skip(p, k) => {
                let pv = &status.parts[part_index(p)];
                let Some(&(name, ..)) = optional(p).get(k) else { return };
                let mut skip = pv.skip.clone().unwrap_or_else(|| pv.skipped.clone());
                let leaving = !skip.iter().any(|s| s == name);
                // (what a converted pack holds is removed now, after the question)
                if leaving && pv.converted && !pv.skipped.iter().any(|s| s == name) {
                    return self.go(Page::Remove(p, Some(k)), Some(Act::Back), status);
                }
                if leaving {
                    skip.push(name.into());
                } else {
                    skip.retain(|s| s != name);
                }
                self.commands.push(Command::Skip(p, skip));
            }
            Act::MainGame => self.commands.push(Command::MainGame(if status.main == Part::T3 { Part::Ttt } else { Part::T3 })),
            Act::Seen => {
                self.log = false;
                self.commands.push(Command::Seen);
                // (back where the player was: the game converted, or the one that failed)
                if let Some(Kind::Convert(p)) = status.outcome.as_ref().map(|o| o.kind) {
                    self.page = Page::Game(p);
                    self.focus = 0;
                }
            }
        }
    }

    /// The rows of the page shown now.
    fn rows(&self, status: &Status) -> Vec<Row> {
        let row = |text: &str, on: bool, act| Row { text: text.into(), on, act, right: None };
        let busy = status.job.is_some() || status.copy.is_some();
        if self.log {
            let mut v = Vec::new();
            if status.can.copy {
                v.push(row("COPY", true, Act::Copy));
            }
            v.push(row("BACK", true, Act::Details));
            return v;
        }
        if let Some(j) = &status.job {
            return vec![row("DETAILS", true, Act::Details), row(if j.cancelling { "CANCELLING" } else { "CANCEL" }, j.cancellable && !j.cancelling, Act::Cancel)];
        }
        if status.copy.is_some() {
            return Vec::new();
        }
        if status.pending.is_some() {
            return vec![row("CONVERT ANYWAY", true, Act::ConvertAnyway), row("BACK", true, Act::DropPending)];
        }
        if let Some(o) = &status.outcome {
            let mut v = Vec::new();
            if o.ok && status.playable {
                v.push(row("PLAY", true, Act::Play));
            }
            v.push(row("OK", true, Act::Seen));
            v.push(row("DETAILS", true, Act::Details));
            return v;
        }
        let picking = status.picking.is_some();
        match self.page {
            Page::Home => {
                let mut v: Vec<Row> = Part::ALL
                    .into_iter()
                    .map(|p| {
                        let pv = &status.parts[part_index(p)];
                        let right = if pv.converted {
                            match (pv.outdated, pv.note.is_some()) {
                                (true, _) => (format!("{} · OUT OF DATE", gb(pv.size)), ORANGE),
                                (_, true) => (format!("{} · PARTLY", gb(pv.size)), ORANGE),
                                _ => (gb(pv.size), GREEN),
                            }
                        } else if p == status.main {
                            ("NEEDED".into(), ORANGE)
                        } else {
                            ("OPTIONAL".into(), GREY)
                        };
                        Row { text: part_title(p).into(), on: !status.loading, act: Act::Part(p), right: Some(right) }
                    })
                    .collect();
                // (a choice once Tekken Tag is converted: its owner may have no Tekken 3)
                if status.parts[part_index(Part::Ttt)].converted {
                    v.push(row(&format!("MAIN GAME: {}", part_title(status.main)), !busy && !status.loading, Act::MainGame));
                }
                if let Some(label) = status.can.import {
                    v.push(row(label, !busy && !picking && !status.loading, Act::Import));
                }
                v.push(row("PLAY", status.playable && !busy, Act::Play));
                if status.can.quit {
                    v.push(row("QUIT", !busy, Act::Quit));
                }
                v
            }
            Page::Game(p) => {
                let pv = &status.parts[part_index(p)];
                let choose = match (p, status.can.folders, pv.converted) {
                    (_, _, true) => "CONVERT AGAIN",
                    (Part::Cs, _, _) | (_, true, _) => "CHOOSE FOLDER",
                    (Part::Arcade, _, _) => "CHOOSE ROM SET",
                    _ => "CHOOSE DISC IMAGE",
                };
                let mut v = vec![row(choose, !busy && !picking, Act::Pick(p))];
                let skip = pv.skip.clone().unwrap_or_else(|| pv.skipped.clone());
                for (k, &(name, label, size)) in optional(p).iter().enumerate() {
                    let has = |l: &[String]| l.iter().any(|s| s == name);
                    let right = match (has(&skip), pv.converted && has(&pv.skipped)) {
                        (true, true) => ("LEFT OUT".to_string(), GREY),
                        (true, false) => (format!("SKIP · SAVES {} MB", size / 1_000_000), GREY),
                        (false, true) => ("ADD BACK: CONVERT AGAIN".into(), ORANGE),
                        (false, false) => (format!("KEEP · {} MB", size / 1_000_000), GREEN),
                    };
                    v.push(Row { text: label.into(), on: !busy && !picking, act: Act::Skip(p, k), right: Some(right) });
                }
                if pv.converted {
                    v.push(row("REMOVE", !busy && !picking, Act::AskRemove(p, None)));
                }
                v.push(row("BACK", true, Act::Back));
                v
            }
            Page::Remove(p, k) => vec![row("REMOVE", !busy, Act::Remove(p, k)), row("KEEP IT", true, Act::Back)],
        }
    }

    fn draw(&mut self, ctx: &egui::Context, status: &Status, rows: &[Row]) {
        let screen = ctx.content_rect();
        let painter = ctx.layer_painter(egui::LayerId::background());
        painter.rect_filled(screen, 0.0, Color32::BLACK);
        // the scale: the game's menus are 640 x 480
        let u = (screen.width() / 640.0).min(screen.height() / 480.0).clamp(0.6, 3.0);
        let t = self.started.elapsed().as_secs_f32();
        backdrop(&painter, screen, u, t);
        let cx = screen.center().x;
        let col = (screen.width() - 32.0).min(600.0 * u);
        let left = cx - col / 2.0;
        let mut y = screen.top() + 26.0 * u;
        // the heading
        let head = if self.log {
            "DETAILS"
        } else if status.job.is_some() {
            "CONVERTING"
        } else if status.copy.is_some() {
            "COPYING"
        } else if status.pending.is_some() {
            "NOT ENOUGH SPACE?"
        } else if let Some(o) = &status.outcome {
            match (o.ok, o.cancelled) {
                (true, _) => "DONE",
                (_, true) => "CANCELLED",
                _ => "IT DID NOT WORK",
            }
        } else {
            match self.page {
                Page::Home => "GAME DATA",
                Page::Game(p) => part_title(p),
                Page::Remove(..) => "REMOVE?",
            }
        };
        let r = painter.text(pos2(cx, y), Align2::CENTER_TOP, head, display(46.0 * u), GOLD);
        y = r.bottom() + 4.0 * u;
        rule(&painter, cx, y, col * 0.6, u);
        y += 14.0 * u;
        let wrap = |s: &str, size: f32, c: Color32| painter.layout(s.to_string(), text(size), c, col);
        let say = |painter: &egui::Painter, y: &mut f32, s: &str, size: f32, c: Color32| {
            let g = wrap(s, size, c);
            let h = g.size().y;
            painter.galley(pos2(cx - g.size().x / 2.0, *y), g, c);
            *y += h + 6.0 * u;
        };
        let small = (17.0 * u).max(13.0);
        self.drop_targets.clear();
        if self.log {
            let log: &[String] = status.job.as_ref().map(|j| &j.log[..]).or(status.outcome.as_ref().map(|o| &o.log[..])).unwrap_or(&[]);
            let bottom = screen.bottom() - (rows.len() as f32 * 40.0 + 60.0) * u;
            self.log_box(&painter, Rect::from_min_max(pos2(left, y), pos2(left + col, bottom.max(y + 60.0))), log, u);
            y = bottom.max(y + 60.0) + 12.0 * u;
        } else if let Some(j) = &status.job {
            let what = match j.kind {
                Kind::Convert(p) => part_title(p).to_string(),
                Kind::Install => "INSTALLING THE PACKS".into(),
                Kind::Remove => "REMOVING".into(),
            };
            say(&painter, &mut y, &what, 26.0 * u, WHITE);
            let (frac, doing, left_s) = match &self.progress {
                Some((_, p, _)) => (p.fraction(j.seconds), p.doing.clone(), p.left(j.seconds)),
                None => (0.0, String::new(), None),
            };
            y += 8.0 * u;
            let pct = painter.text(pos2(cx, y), Align2::CENTER_TOP, format!("{}%", (frac * 100.0).floor() as u32), display(64.0 * u), WHITE);
            y = pct.bottom() + 8.0 * u;
            bar(&painter, Rect::from_min_size(pos2(left, y), vec2(col, 22.0 * u)), frac, t, u);
            y += 34.0 * u;
            let times = match left_s {
                Some(l) => format!("{} elapsed · about {} left", clock(j.seconds), clock(l)),
                None => format!("{} elapsed", clock(j.seconds)),
            };
            say(&painter, &mut y, &times, small, GREY);
            let doing = if j.cancelling {
                "Stopping at the next safe point…".to_string()
            } else if doing.is_empty() {
                j.log.last().cloned().unwrap_or_default()
            } else {
                doing
            };
            say(&painter, &mut y, &doing, small, WHITE);
            say(&painter, &mut y, "Keep OpenKumite open. Nothing is uploaded: it all happens on this device.", small, DIM);
        } else if let Some((done, all)) = status.copy {
            say(&painter, &mut y, "The chosen files are copied to the app first.", small, GREY);
            let frac = if all > 0 { done as f32 / all as f32 } else { 0.0 };
            let pct = painter.text(pos2(cx, y), Align2::CENTER_TOP, format!("{}%", (frac * 100.0).floor() as u32), display(64.0 * u), WHITE);
            y = pct.bottom() + 8.0 * u;
            bar(&painter, Rect::from_min_size(pos2(left, y), vec2(col, 22.0 * u)), frac, t, u);
            y += 34.0 * u;
            say(&painter, &mut y, &format!("{} of {}", gb(done), gb(all)), small, GREY);
        } else if let Some(p) = &status.pending {
            // (a browser gives a site an allowance, often less than the disk's free space; rounded so that the space
            // short never reads as enough: the need up, the free space down)
            let place = if cfg!(target_arch = "wasm32") { "this browser lets the game use" } else { "this device has" };
            let need = p.needed.div_ceil(100_000_000) * 100_000_000;
            let free = p.free / 100_000_000 * 100_000_000;
            say(&painter, &mut y, &format!("Converting {} needs about {} free; {place} {}.", title_case(part_title(p.part)), gb(need), gb(free)), small + 2.0, WHITE);
            say(&painter, &mut y, &format!("Image: {}", p.image), small, GREY);
            say(&painter, &mut y, "Free some space and choose the image again, or try anyway: it stops cleanly if the space runs out.", small, GREY);
        } else if let Some(o) = &status.outcome {
            say(
                &painter,
                &mut y,
                &sentence(&o.message),
                small + 2.0,
                if o.ok {
                    WHITE
                } else if o.cancelled {
                    ORANGE
                } else {
                    RED
                },
            );
            if !o.ok && !o.cancelled {
                say(&painter, &mut y, "DETAILS shows the log; it says where it stopped.", small, GREY);
            }
        } else {
            match self.page {
                Page::Home => {
                    say(&painter, &mut y, "OpenKumite plays the games you own. It converts them once, here on this device, and keeps the result. Nothing is uploaded.", small, GREY);
                    if status.loading {
                        say(&painter, &mut y, "Looking at the game data…", small, WHITE);
                    } else if status.main == Part::Ttt && status.parts[part_index(Part::Ttt)].converted && !status.playable {
                        say(&painter, &mut y, &format!("{NOT_YET}."), small, ORANGE);
                    }
                }
                Page::Game(p) => {
                    let pv = &status.parts[part_index(p)];
                    say(&painter, &mut y, part_about(p), small, GREY);
                    if pv.converted {
                        say(&painter, &mut y, &format!("Converted: {} on this device.", gb(pv.size)), small, GREEN);
                        if pv.outdated {
                            say(&painter, &mut y, OUTDATED, small, ORANGE);
                        }
                        if let Some(n) = &pv.note {
                            say(&painter, &mut y, n, small, ORANGE);
                        }
                    } else {
                        say(&painter, &mut y, part_input(p, status.can.folders), small, WHITE);
                        if status.can.drop {
                            say(&painter, &mut y, "Or drop it on this window.", small, DIM);
                        }
                    }
                    if !optional(p).is_empty() {
                        say(&painter, &mut y, "To save space, skip what the game plays without: no ending movie goes straight to the credits, no music plays silent. Adding a part back needs the disc image (CONVERT AGAIN).", small, DIM);
                    }
                    if status.picking == Some(p) {
                        say(&painter, &mut y, "Waiting for the files you choose…", small, GOLD);
                    }
                }
                Page::Remove(p, Some(k)) => {
                    let (_, label, size) = optional(p).get(k).copied().unwrap_or_default();
                    say(
                        &painter,
                        &mut y,
                        &format!(
                            "Removes {}'s {} (about {} MB) from this device. The game plays on without; to add back, CONVERT AGAIN from the disc image.",
                            title_case(part_title(p)),
                            label.to_lowercase(),
                            size / 1_000_000
                        ),
                        small + 2.0,
                        WHITE,
                    );
                }
                Page::Remove(p, None) => {
                    let pv = &status.parts[part_index(p)];
                    say(&painter, &mut y, &format!("{} ({}) is deleted from this device. You can convert it again from your copy later.", title_case(part_title(p)), gb(pv.size)), small + 2.0, WHITE);
                    if p == Part::T3 {
                        say(&painter, &mut y, "Without Tekken 3 the game cannot start until it is converted again.", small, ORANGE);
                    }
                }
            }
        }
        if let Some(m) = &status.message {
            say(&painter, &mut y, &sentence(m), small, RED);
        }
        // the rows: centred, the chosen one on the menu's bar
        let row_h = 40.0 * u;
        let n = rows.len() as f32;
        let mut top = (y + 10.0 * u).max(screen.bottom() - 34.0 * u - n * row_h - 8.0 * u);
        if matches!(self.page, Page::Home) && status.job.is_none() && status.outcome.is_none() && status.pending.is_none() && !self.log {
            top = y + 8.0 * u;
        }
        // (rows that would run under the hints: closer together)
        let row_h = ((screen.bottom() - 34.0 * u - top) / n.max(1.0)).clamp(row_h * 0.5, row_h);
        self.hits.clear();
        let pulse = 0.75 + 0.25 * (t * 3.0).sin();
        for (k, r) in rows.iter().enumerate() {
            let rect = Rect::from_min_size(pos2(screen.left(), top + k as f32 * row_h), vec2(screen.width(), row_h));
            self.hits.push(Rect::from_center_size(rect.center(), vec2(col.max(300.0 * u), row_h)));
            let chosen = k == self.focus && r.on;
            if chosen {
                highlight(&painter, rect.shrink2(vec2(0.0, 3.0 * u)), cx, col, pulse);
            }
            let colour = if !r.on {
                DIM
            } else if chosen {
                GOLD
            } else {
                WHITE
            };
            if let Some((note, c)) = &r.right {
                // a game: its name left, its state right, inside the column
                painter.text(pos2(left + 12.0 * u, rect.center().y), Align2::LEFT_CENTER, &r.text, display(28.0 * u), colour);
                painter.text(pos2(left + col - 12.0 * u, rect.center().y), Align2::RIGHT_CENTER, note, text(small), if r.on { *c } else { DIM });
                if let Act::Part(p) = r.act {
                    self.drop_targets.push((rect, p));
                }
            } else {
                painter.text(rect.center(), Align2::CENTER_CENTER, &r.text, display(28.0 * u), colour);
            }
        }
        // files over the window: where they would go
        if self.hovering || status.hovering {
            let at = self.pointer.and_then(|p| self.drop_targets.iter().find(|(r, _)| r.contains(p)).map(|t| t.1));
            let label = match at {
                Some(p) => format!("DROP FOR {}", part_title(p)),
                None => "DROP ON A GAME".into(),
            };
            painter.rect_stroke(screen.shrink(8.0), 12.0, Stroke::new(3.0 * u, GOLD), egui::StrokeKind::Inside);
            painter.text(pos2(cx, screen.bottom() - 60.0 * u), Align2::CENTER_CENTER, label, display(30.0 * u), GOLD);
        }
        // the controls, as the player last used them
        let hint = match self.device {
            Device::Keys => "↑ ↓  CHOOSE     ENTER  SELECT     ESC  BACK",
            Device::Pad => "↑ ↓  CHOOSE     A / CROSS  SELECT     B / CIRCLE  BACK",
            Device::Pointer => "CLICK TO SELECT",
            Device::Touch => "",
        };
        let hint = if self.log { "↑ ↓ / WHEEL / DRAG  SCROLL" } else { hint };
        painter.text(pos2(cx, screen.bottom() - 14.0 * u), Align2::CENTER_BOTTOM, hint, text(small * 0.9), DIM);
        if let Some((s, at)) = &self.toast {
            if at.elapsed().as_secs_f32() < 1.6 {
                painter.text(pos2(cx, screen.bottom() - 40.0 * u), Align2::CENTER_BOTTOM, s, display(22.0 * u), GREEN);
            } else {
                self.toast = None;
            }
        }
    }

    /// The log's lines in a box, the newest at the bottom unless scrolled up.
    fn log_box(&mut self, painter: &egui::Painter, rect: Rect, log: &[String], u: f32) {
        painter.rect_filled(rect, 6.0, Color32::from_rgb(14, 14, 16));
        painter.rect_stroke(rect, 6.0, Stroke::new(1.0, DIM), egui::StrokeKind::Inside);
        let font = FontId::monospace((12.0 * u).max(11.0));
        let line_h = font.size * 1.35;
        let inner = rect.shrink(10.0);
        let rows = ((inner.height() / line_h).floor() as usize).max(1);
        let max_scroll = log.len().saturating_sub(rows) as f32;
        self.scroll = self.scroll.min(max_scroll);
        let end = log.len().saturating_sub(self.scroll as usize);
        let start = end.saturating_sub(rows);
        let clip = painter.with_clip_rect(inner);
        for (k, l) in log[start..end].iter().enumerate() {
            clip.text(pos2(inner.left(), inner.top() + k as f32 * line_h), Align2::LEFT_TOP, l, font.clone(), GREY);
        }
        if log.is_empty() {
            clip.text(inner.center(), Align2::CENTER_CENTER, "(nothing yet)", font, DIM);
        }
    }
}

fn part_index(p: Part) -> usize {
    Part::ALL.iter().position(|q| *q == p).unwrap_or(0)
}

/// A message as a sentence: its first letter a capital (the converters' errors begin in lower case).
fn sentence(s: &str) -> String {
    let mut c = s.chars();
    c.next().map_or(String::new(), |f| f.to_uppercase().collect::<String>() + c.as_str())
}

fn title_case(s: &str) -> String {
    s.split(' ')
        .map(|w| {
            let mut c = w.chars();
            c.next().map_or(String::new(), |f| f.to_string() + &c.as_str().to_lowercase())
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// A key's meaning here (arrows, WASD, Tab, Enter, Space, Esc, Backspace, the page keys; Android's d-pad, its
/// gamepad's A and B, its back).
fn key_nav(physical: &PhysicalKey) -> Option<Nav> {
    if let PhysicalKey::Unidentified(NativeKeyCode::Android(code)) = physical {
        return match code {
            19 => Some(Nav::Up),
            20 => Some(Nav::Down),
            21 => Some(Nav::Up),
            22 => Some(Nav::Down),
            23 | 66 | 96 | 108 => Some(Nav::Go),
            4 | 97 | 109 => Some(Nav::Back),
            _ => None,
        };
    }
    Some(match physical {
        PhysicalKey::Code(KeyCode::ArrowUp | KeyCode::KeyW | KeyCode::ArrowLeft | KeyCode::KeyA) => Nav::Up,
        PhysicalKey::Code(KeyCode::ArrowDown | KeyCode::KeyS | KeyCode::ArrowRight | KeyCode::KeyD | KeyCode::Tab) => Nav::Down,
        PhysicalKey::Code(KeyCode::Enter | KeyCode::NumpadEnter | KeyCode::Space) => Nav::Go,
        PhysicalKey::Code(KeyCode::Escape | KeyCode::Backspace | KeyCode::BrowserBack) => Nav::Back,
        PhysicalKey::Code(KeyCode::PageUp) => Nav::PageUp,
        PhysicalKey::Code(KeyCode::PageDown) => Nav::PageDown,
        PhysicalKey::Code(KeyCode::Home) => Nav::Top,
        PhysicalKey::Code(KeyCode::End) => Nav::End,
        _ => return None,
    })
}

/// The background: black, a slow band of light across, as the game's menus behind their lists.
fn backdrop(painter: &egui::Painter, screen: Rect, u: f32, t: f32) {
    let band = 180.0 * u;
    let y = screen.top() + screen.height() * (0.3 + 0.05 * (t * 0.2).sin());
    let c = Color32::from_rgb(28, 22, 14);
    let mut mesh = egui::Mesh::default();
    let (l, r) = (screen.left(), screen.right());
    for (k, (yy, col)) in [(y - band, Color32::BLACK), (y, c), (y + band, Color32::BLACK)].into_iter().enumerate() {
        mesh.colored_vertex(pos2(l, yy), col);
        mesh.colored_vertex(pos2(r, yy), col);
        if k > 0 {
            let b = (k as u32) * 2;
            mesh.add_triangle(b - 2, b - 1, b);
            mesh.add_triangle(b - 1, b, b + 1);
        }
    }
    painter.add(Shape::mesh(mesh));
}

/// A rule under the heading: gold, fading to both ends.
fn rule(painter: &egui::Painter, cx: f32, y: f32, w: f32, u: f32) {
    let h = 2.0 * u;
    let mut mesh = egui::Mesh::default();
    for (x, c) in [(cx - w / 2.0, Color32::TRANSPARENT), (cx, GOLD), (cx + w / 2.0, Color32::TRANSPARENT)] {
        mesh.colored_vertex(pos2(x, y), c);
        mesh.colored_vertex(pos2(x, y + h), c);
    }
    for b in [0u32, 2] {
        mesh.add_triangle(b, b + 1, b + 2);
        mesh.add_triangle(b + 1, b + 2, b + 3);
    }
    painter.add(Shape::mesh(mesh));
}

/// The menu's highlight bar (as the game draws it: shaded black to grey to black across, a lighter line at its top
/// and bottom), its light pulsing.
fn highlight(painter: &egui::Painter, rect: Rect, cx: f32, col: f32, pulse: f32) {
    let grey = |v: f32| Color32::from_gray((v * pulse).clamp(0.0, 255.0) as u8);
    let half = (col * 0.75).max(rect.width() * 0.3);
    let shade = |painter: &egui::Painter, r: Rect, mid: Color32| {
        let mut mesh = egui::Mesh::default();
        for (x, c) in [(cx - half, Color32::BLACK), (cx, mid), (cx + half, Color32::BLACK)] {
            mesh.colored_vertex(pos2(x, r.top()), c);
            mesh.colored_vertex(pos2(x, r.bottom()), c);
        }
        for b in [0u32, 2] {
            mesh.add_triangle(b, b + 1, b + 2);
            mesh.add_triangle(b + 1, b + 2, b + 3);
        }
        painter.add(Shape::mesh(mesh));
    };
    shade(painter, rect, grey(96.0));
    let line = (rect.height() * 0.06).max(1.0);
    shade(painter, Rect::from_min_size(rect.min, vec2(rect.width(), line)), grey(190.0));
    shade(painter, Rect::from_min_size(pos2(rect.left(), rect.bottom() - line), vec2(rect.width(), line)), grey(190.0));
}

/// A progress bar in the game's HUD manner: a dark bevelled track, a gold fill with a moving sheen.
fn bar(painter: &egui::Painter, rect: Rect, frac: f32, t: f32, u: f32) {
    painter.rect_filled(rect, 3.0 * u, Color32::from_rgb(24, 22, 20));
    painter.rect_stroke(rect, 3.0 * u, Stroke::new(1.5 * u, Color32::from_rgb(90, 84, 70)), egui::StrokeKind::Outside);
    let inner = rect.shrink(3.0 * u);
    let w = inner.width() * frac.clamp(0.0, 1.0);
    if w > 0.5 {
        let fill = Rect::from_min_size(inner.min, vec2(w, inner.height()));
        let mut mesh = egui::Mesh::default();
        let (top, low) = (Color32::from_rgb(255, 214, 102), Color32::from_rgb(214, 120, 30));
        mesh.colored_vertex(fill.left_top(), top);
        mesh.colored_vertex(fill.right_top(), top);
        mesh.colored_vertex(fill.left_bottom(), low);
        mesh.colored_vertex(fill.right_bottom(), low);
        mesh.add_triangle(0, 1, 2);
        mesh.add_triangle(1, 2, 3);
        painter.add(Shape::mesh(mesh));
        // the sheen: a light stripe running along the fill
        let x = fill.left() + (t * 0.5).fract() * (fill.width() + 60.0 * u) - 30.0 * u;
        let sheen = Rect::from_min_max(pos2(x.max(fill.left()), fill.top()), pos2((x + 30.0 * u).min(fill.right()), fill.bottom()));
        if sheen.width() > 0.0 {
            painter.rect_filled(sheen, 0.0, Color32::from_white_alpha(46));
        }
    }
    // segments, as a health bar's
    let n = 20;
    for k in 1..n {
        let x = inner.left() + inner.width() * k as f32 / n as f32;
        painter.line_segment([pos2(x, inner.top()), pos2(x, inner.bottom())], Stroke::new(1.0, Color32::from_black_alpha(90)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status() -> Status {
        Status { can: Abilities { quit: true, copy: true, drop: true, ..Default::default() }, ..Default::default() }
    }

    fn run(s: &mut Screen, st: &Status) -> Vec<Command> {
        let ctx = egui::Context::default();
        let mut out = Vec::new();
        let mut o = ctx.run_ui(egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1280.0, 720.0))), ..Default::default() }, |ui| {
            out = s.frame(ui.ctx(), st);
        });
        // (no renderer takes the fonts' textures: dropped on purpose, which egui checks in debug builds)
        o.textures_delta.clear();
        out
    }

    #[test]
    fn what_the_copy_did_not_hold() {
        assert_eq!(missing_note(&serde_json::json!({"source": {"file": "t.bin"}})), None);
        assert_eq!(missing_note(&serde_json::json!({"source": {"missing": []}})), None);
        let note = missing_note(&serde_json::json!({"source": {"missing": ["movies", "XA music", "CD audio"]}})).unwrap();
        assert!(note.starts_with("Without movies, XA music and CD audio: "), "{note}");
    }

    /// An optional part: skipped for the next conversion; on a converted pack, removed after the question; one its
    /// pack left out, added back by the next conversion.
    #[test]
    fn an_optional_part_is_skipped_or_removed() {
        let mut s = Screen::new();
        let mut st = status();
        s.page = Page::Game(Part::T3);
        run(&mut s, &st);
        s.nav.extend([Nav::Down, Nav::Go]);
        assert_eq!(run(&mut s, &st), vec![Command::Skip(Part::T3, vec!["endings".into()])]);
        let mut s = Screen::new();
        st.parts[0] = PartView { converted: true, skipped: vec!["music".into()], ..Default::default() };
        s.page = Page::Game(Part::T3);
        run(&mut s, &st);
        s.nav.extend([Nav::Down, Nav::Go]);
        run(&mut s, &st);
        assert_eq!(s.page, Page::Remove(Part::T3, Some(0)));
        s.nav.extend([Nav::Up, Nav::Go]);
        assert_eq!(run(&mut s, &st), vec![Command::Strip(Part::T3, "endings".into())]);
        assert_eq!(s.page, Page::Game(Part::T3));
        s.nav.extend([Nav::Down, Nav::Down, Nav::Down, Nav::Down, Nav::Go]);
        assert_eq!(run(&mut s, &st), vec![Command::Skip(Part::T3, vec![])]);
    }

    #[test]
    fn the_keys_reach_a_game_and_pick_its_image() {
        let mut s = Screen::new();
        let st = status();
        run(&mut s, &st);
        // HOME: Tekken 3 first; Enter opens it; Enter again picks
        s.key(&PhysicalKey::Code(KeyCode::Enter));
        run(&mut s, &st);
        assert_eq!(s.page, Page::Game(Part::T3));
        s.key(&PhysicalKey::Code(KeyCode::Enter));
        assert_eq!(run(&mut s, &st), vec![Command::Pick(Part::T3)]);
        // Esc back to HOME, on Tekken 3; down twice: Counter-Strike
        s.key(&PhysicalKey::Code(KeyCode::Escape));
        run(&mut s, &st);
        assert_eq!(s.page, Page::Home);
        s.key(&PhysicalKey::Code(KeyCode::ArrowDown));
        s.key(&PhysicalKey::Code(KeyCode::ArrowDown));
        s.key(&PhysicalKey::Code(KeyCode::Enter));
        run(&mut s, &st);
        assert_eq!(s.page, Page::Game(Part::Cs));
    }

    /// MAIN GAME once Tekken Tag is converted, with or without Tekken 3; it asks for the other one.
    #[test]
    fn the_main_game_row_switches_to_the_other_game() {
        let mut s = Screen::new();
        let mut st = status();
        st.parts[0].converted = true;
        assert!(!s.rows(&st).iter().any(|r| r.act == Act::MainGame), "Tekken 3 alone: no row");
        st.parts[4].converted = true;
        let k = s.rows(&st).iter().position(|r| r.act == Act::MainGame).unwrap();
        assert_eq!(s.rows(&st)[k].text, "MAIN GAME: TEKKEN 3");
        run(&mut s, &st);
        s.focus = k;
        s.key(&PhysicalKey::Code(KeyCode::Enter));
        assert_eq!(run(&mut s, &st), vec![Command::MainGame(Part::Ttt)]);
        st.parts[0].converted = false;
        assert!(s.rows(&st).iter().any(|r| r.act == Act::MainGame), "Tekken Tag alone: the row");
    }

    #[test]
    fn play_is_offered_only_when_the_game_can_start_and_quit_ends() {
        let mut s = Screen::new();
        let mut st = status();
        run(&mut s, &st);
        // (PLAY not choosable: up from Tekken 3 wraps to QUIT)
        s.key(&PhysicalKey::Code(KeyCode::ArrowUp));
        s.key(&PhysicalKey::Code(KeyCode::Enter));
        assert_eq!(run(&mut s, &st), vec![Command::Quit]);
        st.playable = true;
        st.parts[0].converted = true;
        s.key(&PhysicalKey::Code(KeyCode::ArrowUp));
        s.key(&PhysicalKey::Code(KeyCode::Enter));
        assert_eq!(run(&mut s, &st), vec![Command::Play]);
    }

    #[test]
    fn a_job_shows_its_page_and_cancels() {
        let mut s = Screen::new();
        let mut st = status();
        st.job = Some(JobView { kind: Kind::Convert(Part::T3), seconds: 10.0, log: vec!["[characters] export_gltf -> characters".into()], cancellable: true, cancelling: false });
        run(&mut s, &st);
        // DETAILS, CANCEL
        s.key(&PhysicalKey::Code(KeyCode::ArrowDown));
        s.key(&PhysicalKey::Code(KeyCode::Enter));
        assert_eq!(run(&mut s, &st), vec![Command::Cancel]);
        // the outcome, then OK: back on the game's page
        st.job = None;
        st.outcome = Some(Outcome { kind: Kind::Convert(Part::T3), ok: false, cancelled: true, message: "Cancelled".into(), log: Vec::new() });
        run(&mut s, &st);
        s.key(&PhysicalKey::Code(KeyCode::Enter));
        assert_eq!(run(&mut s, &st), vec![Command::Seen]);
        assert_eq!(s.page, Page::Game(Part::T3));
    }

    #[test]
    fn a_click_and_a_tap_choose_the_row_under_them() {
        let mut s = Screen::new();
        let st = status();
        run(&mut s, &st);
        let r = s.hits[1];
        s.event(&WindowEvent::CursorMoved { device_id: winit::event::DeviceId::dummy(), position: winit::dpi::PhysicalPosition::new(r.center().x as f64, r.center().y as f64) });
        s.event(&WindowEvent::MouseInput { device_id: winit::event::DeviceId::dummy(), state: ElementState::Pressed, button: MouseButton::Left });
        s.event(&WindowEvent::MouseInput { device_id: winit::event::DeviceId::dummy(), state: ElementState::Released, button: MouseButton::Left });
        run(&mut s, &st);
        assert_eq!(s.page, Page::Game(Part::T2));
        // a tap on BACK (the last row)
        let r = *s.hits.last().unwrap();
        for phase in [TouchPhase::Started, TouchPhase::Ended] {
            s.event(&WindowEvent::Touch(winit::event::Touch {
                device_id: winit::event::DeviceId::dummy(),
                phase,
                location: winit::dpi::PhysicalPosition::new(r.center().x as f64, r.center().y as f64),
                force: None,
                id: 1,
            }));
        }
        run(&mut s, &st);
        assert_eq!(s.page, Page::Home);
    }

    #[test]
    fn a_drop_goes_to_the_game_under_the_pointer() {
        let mut s = Screen::new();
        let st = status();
        run(&mut s, &st);
        let r = s.drop_targets.iter().find(|t| t.1 == Part::T2).unwrap().0;
        s.event(&WindowEvent::CursorMoved { device_id: winit::event::DeviceId::dummy(), position: winit::dpi::PhysicalPosition::new(r.center().x as f64, r.center().y as f64) });
        s.event(&WindowEvent::DroppedFile(PathBuf::from("/x/t2.chd")));
        assert_eq!(run(&mut s, &st), vec![Command::Drop(Part::T2, vec![PathBuf::from("/x/t2.chd")])]);
    }

    /// Every page drawn as the platforms will show it, into PNGs (`OK_SHOTS=<folder> cargo test --release -p
    /// openkumite -- --ignored game_data_screenshots`), at 1280 x 720 and a phone's 800 x 360.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    #[ignore]
    fn game_data_screenshots() {
        let dir = std::path::PathBuf::from(std::env::var("OK_SHOTS").unwrap());
        std::fs::create_dir_all(&dir).unwrap();
        let mut st = status();
        let t3_log: Vec<String> = [
            "Tekken_3_(USA).cue (cue): track 1 data, 268934 sectors; track 2 audio, 11778 sectors",
            "[characters] export_gltf -> characters",
            "[characters] done",
            "[stages] export_stages -> stages",
            "[stages] done",
            "[pictures] export_pictures -> pictures",
            "[pictures] done",
            "[media] export_media -> .",
            "sfx banks 51 samples 857",
            "music tracks 51",
            "video 00 sectors 122000-126859 frames 648 fps 20 32.2s",
            "video 01 sectors 126860-130301 frames 459 fps 20 22.9s",
            "video 02 sectors 130302-133000 frames 300 fps 15 20.0s",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let mut shots: Vec<(&str, Status, Vec<Nav>, bool)> = Vec::new();
        shots.push(("1-home-empty", st.clone(), vec![], false));
        shots.push(("2-tekken3-page", st.clone(), vec![Nav::Go], false));
        let mut job = st.clone();
        job.job = Some(JobView { kind: Kind::Convert(Part::T3), seconds: 41.0, log: t3_log.clone(), cancellable: true, cancelling: false });
        shots.push(("3-converting", job.clone(), vec![], false));
        shots.push(("4-details", job, vec![Nav::Go], false));
        let mut pend = st.clone();
        pend.pending = Some(Pending { part: Part::T2, image: "Tekken 2 (USA) (v1.1).7z".into(), needed: 2_700_000_000, free: 1_100_000_000 });
        shots.push(("5-space", pend, vec![], false));
        st.parts[0] = PartView {
            converted: true,
            size: 1_400_000_000,
            note: Some("Without movies and XA music: the disc image did not hold them. Convert again from a full image (.cue with its .bin files, .chd, ...) to add them.".into()),
            outdated: false,
            ..Default::default()
        };
        st.parts[1] = PartView { converted: true, size: 1_700_000_000, note: None, outdated: false, ..Default::default() };
        st.playable = true;
        let mut done = st.clone();
        done.outcome = Some(Outcome { kind: Kind::Convert(Part::T2), ok: true, cancelled: false, message: "Tekken 2 is converted: its fighters are in the roster.".into(), log: t3_log.clone() });
        shots.push(("6-done", done, vec![], false));
        let mut failed = st.clone();
        failed.outcome = Some(Outcome {
            kind: Kind::Convert(Part::Cs),
            ok: false,
            cancelled: false,
            message: "this is the Counter-Strike installer CD: install or unpack the game first, then give its folder".into(),
            log: t3_log,
        });
        shots.push(("7-failed", failed, vec![], false));
        shots.push(("8-home-converted", st.clone(), vec![], true));
        shots.push(("9-remove", st.clone(), vec![Nav::Down, Nav::Go, Nav::Down, Nav::Go], false));
        let mut ttt = st.clone();
        ttt.parts[4] = PartView { converted: true, size: 1_700_000_000, note: None, outdated: false, ..Default::default() };
        ttt.main = Part::Ttt;
        ttt.playable = false;
        shots.push(("10-main-game", ttt, vec![], false));
        let mut old = st.clone();
        old.parts[0].outdated = true;
        shots.push(("11-outdated", old.clone(), vec![], false));
        shots.push(("12-outdated-page", old, vec![Nav::Go], false));
        // what a conversion leaves out: chosen before converting; a converted pack's removed after the question
        let mut slim = status();
        slim.parts[0].skip = Some(vec!["endings".into(), "opening".into()]);
        shots.push(("13-skip-before-converting", slim, vec![Nav::Go, Nav::Down], false));
        let mut slim = st.clone();
        slim.parts[4] = PartView { converted: true, size: 680_000_000, skipped: vec!["endings".into(), "opening".into()], skip: Some(vec!["opening".into()]), ..Default::default() };
        shots.push(("14-converted-slim", slim.clone(), vec![Nav::Down, Nav::Down, Nav::Down, Nav::Down, Nav::Go, Nav::Down], false));
        shots.push(("15-remove-music", slim, vec![Nav::Down, Nav::Down, Nav::Down, Nav::Down, Nav::Go, Nav::Down, Nav::Down, Nav::Down, Nav::Down, Nav::Go], false));
        for (w, h, tag) in [(1280u32, 720u32, ""), (800, 360, "-phone")] {
            let mut r = kumite_render::Renderer::headless(w, h).unwrap();
            for (name, st, navs, hover) in &shots {
                let mut s = Screen::new();
                let mut egui = crate::debug::EguiLayer::new(r.device(), r.overlay_format(), None);
                // (two frames: the rows laid out, then the keys on them)
                for k in 0..3 {
                    if k == 1 {
                        s.nav.extend(navs.iter().copied());
                        if *hover {
                            s.device = Device::Pointer;
                        }
                    }
                    let mut frame = egui.run(None, [w, h], |ctx| {
                        s.frame(ctx, st);
                    });
                    let cam = kumite_render::Camera::new(glam::Vec3::Z, glam::Vec3::ZERO, 1.0);
                    r.render_with(&cam, &[], &[], &[], None, &[], Some(&mut frame)).unwrap();
                }
                let img = r.capture().unwrap();
                crate::write_png(&dir.join(format!("{name}{tag}.png")), &img).unwrap();
            }
        }
    }
}
