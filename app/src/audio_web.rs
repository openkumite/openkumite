//! Audio in a browser: the converted FLAC / WAV files played by HTML audio elements from the page's server, or from
//! a conversion kept in the browser (`kumite_fs::src`) (the same API as the native player). Browsers start sound only
//! after the first key press or touch; some (Safari on iPhone) only on elements first played in one, so the page
//! unlocks a music element and a pool for the sounds on the first touch (`window.__okaudio`), and those are the ones
//! played. A track that starts or loops from a point of its own (Tekken Tag's) plays through Web Audio instead
//! (`__okaudio.loop`: a buffer source, which loops seamlessly from `loopStart`).

use kumite_game::sound::Track;
use std::path::Path;
use wasm_bindgen::JsCast;
use web_sys::HtmlAudioElement;

/// The page's unlocked elements (`window.__okaudio = { music, sfx: [...] }`), if it has them.
fn page_elements() -> Option<(HtmlAudioElement, Vec<HtmlAudioElement>)> {
    let w = web_sys::window()?;
    let a = js_sys::Reflect::get(&w, &"__okaudio".into()).ok().filter(|v| v.is_object())?;
    let music = js_sys::Reflect::get(&a, &"music".into()).ok()?.dyn_into().ok()?;
    let sfx: js_sys::Array = js_sys::Reflect::get(&a, &"sfx".into()).ok()?.dyn_into().ok()?;
    Some((music, sfx.iter().filter_map(|e| e.dyn_into().ok()).collect()))
}

pub struct Audio {
    music: Option<HtmlAudioElement>,
    /// The Web Audio track playing (`__okaudio.loop`'s handle: `stop()`, `volume(v)`).
    looper: Option<wasm_bindgen::JsValue>,
    /// The page's sound elements, played in turn (none: a new element a sound).
    pool: Vec<HtmlAudioElement>,
    next: usize,
    /// The track asked for, and whether it has started (it waits for the first key press).
    current: Option<Track>,
    started: bool,
    pub muted: bool,
    pub music_volume: f32,
    pub sfx_volume: f32,
}

fn url(path: &Path) -> String {
    kumite_fs::src(path)
}

/// The page may play sound: the player has clicked, touched or pressed a key (browsers block sound before that):
/// `navigator.userActivation`, or where a browser has none, the page's own flag (`window.__okgesture`).
fn allowed() -> bool {
    let Some(w) = web_sys::window() else { return false };
    let nav = w.navigator();
    let has = js_sys::Reflect::get(&nav, &"userActivation".into()).is_ok_and(|v| v.is_object());
    if has {
        return nav.user_activation().has_been_active();
    }
    js_sys::Reflect::get(&w, &"__okgesture".into()).is_ok_and(|v| v.is_truthy())
}

impl Audio {
    pub fn new() -> Option<Self> {
        Some(Self { music: None, looper: None, pool: Vec::new(), next: 0, current: None, started: false, muted: false, music_volume: 0.55, sfx_volume: 0.9 })
    }

    /// Play a track from its start, looped from its loop point or once, at its level; None stops the music.
    pub fn music(&mut self, track: Option<&Track>) {
        if self.current.as_ref() == track {
            return;
        }
        if let Some(m) = &self.music {
            let _ = m.pause();
        }
        if let Some(l) = self.looper.take() {
            call(&l, "stop", &[]);
        }
        self.current = track.cloned();
        self.started = false;
        self.resume();
    }

    /// The music's volume: muted, ducked, the track's level.
    fn level(&self, ducked: bool) -> f64 {
        let gain = self.current.as_ref().map_or(1.0, |t| t.gain);
        if self.muted {
            0.0
        } else {
            (self.music_volume * gain * if ducked { 0.3 } else { 1.0 }).clamp(0.0, 1.0) as f64
        }
    }

    /// The music playing at a volume.
    fn set_level(&self, v: f64) {
        if let Some(l) = &self.looper {
            call(l, "volume", &[v.into()]);
        } else if let Some(m) = &self.music {
            m.set_volume(v);
        }
    }

    /// Start the asked-for track once the page may play sound (called every frame).
    pub fn resume(&mut self) {
        if self.started || !allowed() {
            return;
        }
        self.started = true;
        let Some(t) = self.current.clone() else { return };
        let path = &t.file;
        // from a point of its own: Web Audio
        let looper = page_elements().and_then(|_| js_sys::Reflect::get(&web_sys::window()?.into(), &"__okaudio".into()).ok());
        let looper = looper.and_then(|a| Some((js_sys::Reflect::get(&a, &"loop".into()).ok()?.dyn_into::<js_sys::Function>().ok()?, a)));
        if let (Some((f, a)), true) = (looper, t.start > 0.0 || (t.looped && t.loop_at > 0.0)) {
            let args = js_sys::Array::of5(&url(path).into(), &t.start.into(), &t.loop_at.into(), &t.looped.into(), &self.level(false).into());
            self.looper = f.apply(&a, &args).ok();
            return;
        }
        // the page's unlocked element (its source changed), else a new one
        let page = page_elements();
        if let Some((music, pool)) = page.filter(|_| self.pool.is_empty() || self.music.is_some()) {
            if self.pool.is_empty() {
                self.pool = pool;
            }
            self.music.get_or_insert(music);
        }
        let m = match self.music.clone() {
            Some(m) if page_elements().is_some() => {
                m.set_src(&url(path));
                m
            }
            _ => match HtmlAudioElement::new_with_src(&url(path)) {
                Ok(m) => m,
                Err(_) => return,
            },
        };
        m.set_loop(t.looped);
        m.set_volume(self.level(false));
        quiet(m.play());
        self.music = Some(m);
    }

    /// Start a one-shot sound at a speed (1: its own rate; its pitch goes with it).
    pub fn sfx(&mut self, path: &Path, volume: f32, speed: f32) {
        if self.muted || !allowed() {
            return;
        }
        if self.pool.is_empty() {
            if let Some((_, pool)) = page_elements() {
                self.pool = pool;
            }
        }
        // the page's unlocked elements in turn, else a new one
        let s = match self.pool.get(self.next % self.pool.len().max(1)) {
            Some(e) => {
                self.next += 1;
                e.set_src(&url(path));
                e.clone()
            }
            None => match HtmlAudioElement::new_with_src(&url(path)) {
                Ok(e) => e,
                Err(_) => return,
            },
        };
        s.set_volume((volume * self.sfx_volume).clamp(0.0, 1.0) as f64);
        // (an element keeps the pitch at another rate unless told; a new source resets the rate to the default)
        let _ = js_sys::Reflect::set(&s, &"preservesPitch".into(), &false.into());
        s.set_default_playback_rate(speed as f64);
        s.set_playback_rate(speed as f64);
        quiet(s.play());
    }

    pub fn toggle_mute(&mut self) -> bool {
        self.muted = !self.muted;
        self.set_level(self.level(false));
        self.muted
    }

    /// Music quieter while paused.
    pub fn duck(&mut self, ducked: bool) {
        self.set_level(self.level(ducked));
    }
}

/// `handle.name(args)` of a JavaScript object, its result dropped.
fn call(handle: &wasm_bindgen::JsValue, name: &str, args: &[wasm_bindgen::JsValue]) {
    if let Some(f) = js_sys::Reflect::get(handle, &name.into()).ok().and_then(|f| f.dyn_into::<js_sys::Function>().ok()) {
        let _ = f.apply(handle, &args.iter().collect());
    }
}

/// A `play()` interrupted (the music changed at once: nothing to report) or refused (the log says why; the page
/// shows unhandled rejections as errors).
fn quiet(p: Result<js_sys::Promise, wasm_bindgen::JsValue>) {
    thread_local! {
        static IGNORE: wasm_bindgen::closure::Closure<dyn FnMut(wasm_bindgen::JsValue)> = wasm_bindgen::closure::Closure::new(|e: wasm_bindgen::JsValue| {
            let name = js_sys::Reflect::get(&e, &"name".into()).ok().and_then(|n| n.as_string()).unwrap_or_default();
            if name != "AbortError" {
                log::warn!("sound refused: {name} {}", js_sys::Reflect::get(&e, &"message".into()).ok().and_then(|m| m.as_string()).unwrap_or_default());
            }
        });
    }
    if let Ok(p) = p {
        IGNORE.with(|f| {
            let _ = p.catch(f);
        });
    }
}
