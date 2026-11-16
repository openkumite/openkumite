//! The Game data screen's host in a browser: the page (web/index.html, `window.okData`) picks the files, keeps them,
//! runs the converter (convert_worker.js: kumite_convert_web, into the site's storage) and keeps its log; this reads
//! its state each frame (`okData.snapshot()`, JSON) and passes the player's commands on. Playing after anything
//! changed starts the page again (it loads the game data the browser keeps as it starts).

use super::progress::Kind;
use super::{Abilities, Command, JobView, Outcome, PartView, Pending, Status};
use serde_json::Value;
use wasm_bindgen::{JsCast, JsValue};

/// The games (natively `kumite_packs::Part`; the browser's game has no converters in it: their names only).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Part {
    T3,
    T2,
    Cs,
}

impl Part {
    pub const ALL: [Part; 3] = [Part::T3, Part::T2, Part::Cs];

    /// Its short name (the page's, the packs').
    pub fn kind(self) -> &'static str {
        match self {
            Part::T3 => "t3",
            Part::T2 => "t2",
            Part::Cs => "cs",
        }
    }
}

/// What the browser can do on the Game data screen: copy the log; files dropped on the page; a folder kumite_converter
/// wrote taken as it is.
const CAN: Abilities = Abilities { quit: false, folders: false, copy: true, drop: true, import: Some("USE A CONVERTED FOLDER") };

pub struct WebHost {
    last: Value,
}

fn bridge() -> Option<js_sys::Object> {
    let w = web_sys::window()?;
    js_sys::Reflect::get(&w, &"okData".into()).ok()?.dyn_into().ok()
}

/// `okData[name](...args)`; what it gives back.
fn call(name: &str, args: &[JsValue]) -> Option<JsValue> {
    let o = bridge()?;
    let f: js_sys::Function = js_sys::Reflect::get(&o, &name.into()).ok()?.dyn_into().ok()?;
    let a = js_sys::Array::new();
    for v in args {
        a.push(v);
    }
    f.apply(&o, &a).map_err(|e| log::error!("okData.{name}: {e:?}")).ok()
}

fn part_of(s: &str) -> Option<Part> {
    Part::ALL.into_iter().find(|p| p.kind() == s)
}

fn kind_of(v: &Value) -> Kind {
    match v["kind"].as_str().unwrap_or_default() {
        "convert" => Kind::Convert(part_of(v["part"].as_str().unwrap_or_default()).unwrap_or(Part::T3)),
        "remove" => Kind::Remove,
        _ => Kind::Install,
    }
}

fn lines(v: &Value) -> Vec<String> {
    v.as_array().into_iter().flatten().filter_map(|l| l.as_str().map(str::to_string)).collect()
}

/// The page's Game data button pressed since the last ask (`okData.takeOpen()`).
pub fn open_asked() -> bool {
    call("takeOpen", &[]).is_some_and(|v| v.is_truthy())
}

/// Whether the page started without game data (`window.okNoData`): the Game data screen first.
pub fn no_data() -> bool {
    web_sys::window().and_then(|w| js_sys::Reflect::get(&w, &"okNoData".into()).ok()).is_some_and(|v| v.is_truthy())
}

impl Default for WebHost {
    fn default() -> Self {
        Self::new()
    }
}

impl WebHost {
    pub fn new() -> Self {
        call("active", &[JsValue::TRUE]);
        Self { last: Value::Null }
    }

    /// The page's state, as the screen shows it.
    pub fn status(&mut self) -> Status {
        if let Some(s) = call("snapshot", &[]).and_then(|v| v.as_string()) {
            if let Ok(v) = serde_json::from_str::<Value>(&s) {
                self.last = v;
            }
        }
        let v = &self.last;
        if v.is_null() {
            return Status { loading: true, can: CAN, ..Default::default() };
        }
        let parts = Part::ALL.map(|p| {
            let x = &v["parts"][p.kind()];
            PartView { converted: x["converted"].as_bool().unwrap_or(false), size: x["size"].as_u64().unwrap_or(0), note: super::missing_note(&x["manifest"]) }
        });
        let job = v["job"].as_object().map(|_| {
            let j = &v["job"];
            JobView {
                kind: kind_of(j),
                seconds: j["seconds"].as_f64().unwrap_or(0.0),
                log: lines(&j["log"]),
                cancellable: j["kind"] != "remove",
                cancelling: j["cancelling"].as_bool().unwrap_or(false),
            }
        });
        let outcome = v["outcome"].as_object().map(|_| {
            let o = &v["outcome"];
            Outcome {
                kind: kind_of(o),
                ok: o["ok"].as_bool().unwrap_or(false),
                cancelled: o["cancelled"].as_bool().unwrap_or(false),
                message: o["message"].as_str().unwrap_or_default().to_string(),
                log: lines(&o["log"]),
            }
        });
        let pending = v["pending"].as_object().map(|_| {
            let p = &v["pending"];
            Pending {
                part: part_of(p["part"].as_str().unwrap_or_default()).unwrap_or(Part::T3),
                image: p["image"].as_str().unwrap_or_default().to_string(),
                needed: p["needed"].as_u64().unwrap_or(0),
                free: p["free"].as_u64().unwrap_or(0),
            }
        });
        Status {
            parts,
            playable: v["playable"].as_bool().unwrap_or(false),
            loading: v["loading"].as_bool().unwrap_or(false),
            job,
            outcome,
            pending,
            picking: v["picking"].as_str().and_then(part_of),
            copy: None,
            dropped: v["dropped"].as_bool().unwrap_or(false),
            hovering: v["hovering"].as_bool().unwrap_or(false),
            message: v["message"].as_str().map(str::to_string),
            can: CAN,
        }
    }

    pub fn busy(&self) -> bool {
        self.last["job"].is_object()
    }

    pub fn playable(&self) -> bool {
        self.last["playable"].as_bool().unwrap_or(false)
    }

    /// Play starts the page again (the game data changed here, or there was none as it started).
    pub fn play_reloads(&self) -> bool {
        no_data() || self.last["changed"].as_bool().unwrap_or(false)
    }

    pub fn cancel(&mut self) {
        call("cancel", &[]);
    }

    /// Does the screen's command; Play and Quit back (the screen's host's).
    pub fn command(&mut self, c: Command) -> Option<Command> {
        let part = |p: Part| JsValue::from_str(p.kind());
        match c {
            Command::Pick(p) => drop(call("pick", &[part(p)])),
            Command::Drop(p, _) => drop(call("dropFor", &[part(p)])),
            Command::ConvertAnyway => drop(call("convertAnyway", &[])),
            Command::DropPending => drop(call("dropPending", &[])),
            Command::Remove(p) => drop(call("remove", &[part(p)])),
            Command::Cancel => self.cancel(),
            Command::Seen => drop(call("seen", &[])),
            Command::Import => drop(call("importFolder", &[])),
            Command::Copy(text) => drop(call("copy", &[JsValue::from_str(&text)])),
            other => return Some(other),
        }
        None
    }

    /// The page as it is while the game plays (its pad, its hints).
    pub fn leave(&self) {
        call("active", &[JsValue::FALSE]);
    }

    /// The page started again on the game data it keeps.
    pub fn reload() {
        if let Some(w) = web_sys::window() {
            let _ = w.location().reload();
        }
    }
}
