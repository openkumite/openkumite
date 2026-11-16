//! How far a job has got, from its log: each part's conversion writes the same lines on every platform (the
//! converters' steps, `[install] ...`), so its share done is read from the last milestone seen, weighted by how long
//! each step takes (measured: Tekken 3 natively ~53 s, its XA music and movies most of it; Tekken 2's pack ~50 s,
//! its install ~72 s for 47 models). Between milestones it creeps on by time, never past the next one; the time left
//! comes from the rate so far.

use super::Part;

/// What a job does: convert a part (then install the packs), or install only, or remove.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Convert(Part),
    Install,
    Remove,
}

#[derive(Clone, Debug)]
pub struct Progress {
    kind: Kind,
    /// The share done at the last milestone, and where the next one is.
    at: f32,
    next: f32,
    /// Counted lines of a step (movies, models, levels).
    count: u32,
    /// Seconds since the job started, at the last milestone.
    since: f64,
    /// What it is doing now (for the screen).
    pub doing: String,
}

/// Tekken 3's steps (convert_all::STEPS): where each starts; its music (XA streams) from `MUSIC.0` to `MUSIC.1`, its
/// movies after up to the game data. Natively the characters take a few seconds on every core; a browser converts on
/// one thread (measured there: characters 98 s, music 148 s, movies 51 s of ~310 s).
#[cfg(not(target_arch = "wasm32"))]
const T3: [(&str, f32); 8] = [("[characters]", 0.00), ("[stages]", 0.08), ("[pictures]", 0.09), ("[media]", 0.10), ("[gamedata]", 0.92), ("[effects]", 0.95), ("[theater]", 0.96), ("[shadows]", 0.97)];
#[cfg(not(target_arch = "wasm32"))]
const MUSIC: (f32, f32) = (0.11, 0.52);
#[cfg(target_arch = "wasm32")]
const T3: [(&str, f32); 8] =
    [("[characters]", 0.00), ("[stages]", 0.32), ("[pictures]", 0.33), ("[media]", 0.335), ("[gamedata]", 0.975), ("[effects]", 0.985), ("[theater]", 0.99), ("[shadows]", 0.995)];
#[cfg(target_arch = "wasm32")]
const MUSIC: (f32, f32) = (0.34, 0.81);

impl Progress {
    pub fn new(kind: Kind) -> Self {
        Self { kind, at: 0.0, next: 0.05, count: 0, since: 0.0, doing: String::new() }
    }

    fn mark(&mut self, at: f32, next: f32, t: f64) {
        if at >= self.at {
            self.at = at;
            self.next = next.max(at);
            self.since = t;
        }
    }

    /// A line of the job's log, `t` seconds after it started.
    pub fn line(&mut self, line: &str, t: f64) {
        let l = line.trim();
        if l.is_empty() {
            return;
        }
        if let Some(name) = l.strip_prefix("unpacking ") {
            self.doing = format!("Unpacking {name}");
            return;
        }
        if l.starts_with("[install] the fingerprint") {
            self.mark(0.97, 0.995, t);
            self.doing = "Checking the game data".into();
            return;
        }
        if let Some(part) = l.strip_prefix("[install] ") {
            let start = match self.kind {
                Kind::Convert(Part::T2) => 0.40,
                Kind::Convert(Part::Cs) => 0.75,
                Kind::Convert(Part::T3) => 0.97,
                _ => 0.0,
            };
            self.mark(start, start + 0.05, t);
            self.count = 0;
            self.doing = format!("Installing {part}");
            return;
        }
        match self.kind {
            Kind::Convert(Part::T3) => {
                if let Some(&(step, at)) = T3.iter().find(|(s, _)| l.starts_with(s) && !l.ends_with("done")) {
                    let next = T3.iter().map(|s| s.1).find(|&a| a > at).unwrap_or(0.97);
                    let next = if step == "[media]" { MUSIC.0 } else { next };
                    self.mark(at, next, t);
                    self.doing = step_name(step).into();
                } else if l.starts_with("sfx banks") {
                    self.mark(MUSIC.0, MUSIC.1, t);
                    self.doing = "Music".into();
                } else if l.starts_with("music tracks") {
                    self.mark(MUSIC.1, MUSIC.1 + 0.02, t);
                    self.doing = "Movies".into();
                } else if let Some(n) = l.strip_prefix("video ").and_then(|r| r.get(..2)).and_then(|n| n.parse::<f32>().ok()) {
                    // (22 movies, up to the game data)
                    let end = T3[4].1;
                    let at = MUSIC.1 + (end - MUSIC.1) * ((n + 1.0) / 22.0).min(1.0);
                    self.mark(at, (at + 0.02).min(end), t);
                    self.doing = format!("Movie {}", n as u32 + 1);
                }
            }
            Kind::Convert(Part::T2) => {
                if l.starts_with("[t2] the disc") {
                    self.mark(0.0, 0.01, t);
                    self.doing = "Reading the disc".into();
                } else if l.starts_with("[t2] music") {
                    self.mark(0.01, 0.18, t);
                    self.doing = "Music".into();
                } else if l.starts_with("[t2] endings") {
                    self.mark(0.18, 0.20, t);
                    self.doing = "Endings".into();
                } else if let Some(n) = l.strip_prefix("video ").and_then(|r| r.get(..2)).and_then(|n| n.parse::<f32>().ok()) {
                    // (the endings among the disc's 28 movies)
                    let at = 0.18 + 0.22 * ((n + 1.0) / 28.0).min(1.0);
                    self.mark(at, (at + 0.01).min(0.40), t);
                } else if l.starts_with("model_9") {
                    // (47 models)
                    self.count += 1;
                    let at = 0.40 + 0.57 * (self.count as f32 / 47.0).min(1.0);
                    self.mark(at, (at + 0.012).min(0.97), t);
                    self.doing = format!("Installing: {}", l.split(' ').nth(1).unwrap_or("").to_lowercase());
                }
            }
            Kind::Convert(Part::Cs) => {
                if let Some(level) = l.strip_prefix("[cs] ") {
                    // (the number of levels varies: on towards 0.7, never there)
                    self.count += 1;
                    let at = 0.70 * (1.0 - 0.9f32.powi(self.count as i32));
                    self.mark(at, at + 0.03, t);
                    self.doing = format!("Level {level}");
                } else if l.starts_with("stage_") {
                    self.count += 1;
                    let at = (0.75 + 0.02 * self.count as f32).min(0.95);
                    self.mark(at, at + 0.02, t);
                }
            }
            Kind::Install | Kind::Remove => {}
        }
    }

    /// The share done `t` seconds after the job started: the last milestone's, crept on towards the next by the time
    /// spent since (as if this step took as long as the job so far per share).
    pub fn fraction(&self, t: f64) -> f32 {
        if self.at <= 0.0 {
            // (nothing measured yet: a slow start towards the first milestone)
            return (self.next * (1.0 - (-(t as f32) / 20.0).exp())).min(self.next);
        }
        let rate = self.at as f64 / self.since.max(0.1);
        let crept = self.at as f64 + rate * (t - self.since).max(0.0);
        (crept as f32).min(self.next).clamp(0.0, 0.999)
    }

    /// Seconds left, once enough is done to tell.
    pub fn left(&self, t: f64) -> Option<f64> {
        let f = self.fraction(t) as f64;
        (f >= 0.04 && t > 3.0).then(|| t * (1.0 - f) / f)
    }
}

fn step_name(step: &str) -> &'static str {
    match step {
        "[characters]" => "Characters",
        "[stages]" => "Stages",
        "[pictures]" => "Pictures",
        "[media]" => "Sound effects",
        "[gamedata]" => "Game data",
        "[effects]" => "Effects",
        "[theater]" => "Theater",
        "[shadows]" => "Shadows",
        _ => "",
    }
}

/// `m:ss`
pub fn clock(seconds: f64) -> String {
    let s = seconds.max(0.0).round() as u64;
    format!("{}:{:02}", s / 60, s % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tekken_3_moves_on_with_its_steps() {
        let mut p = Progress::new(Kind::Convert(Part::T3));
        assert!(p.fraction(1.0) < 0.05);
        p.line("[characters] export_gltf -> characters", 0.5);
        p.line("[characters] done", 4.0);
        p.line("[stages] export_stages -> stages", 4.0);
        assert_eq!(p.doing, "Stages");
        let a = p.fraction(4.0);
        assert!((0.07..=0.09).contains(&a), "{a}");
        p.line("sfx banks 51 samples 857", 5.0);
        p.line("music tracks 51", 28.0);
        p.line("video 10 sectors ...", 40.0);
        let b = p.fraction(40.0);
        assert!(b > 0.7 && b < 0.75, "{b}");
        // (never past the next milestone however long a step takes)
        assert!(p.fraction(1000.0) <= 0.74);
        p.line("[install] the fingerprint", 52.0);
        assert!(p.fraction(52.0) >= 0.97);
        assert!(p.left(52.0).is_some_and(|l| l < 5.0));
    }

    #[test]
    fn tekken_2_counts_its_models() {
        let mut p = Progress::new(Kind::Convert(Part::T2));
        p.line("[t2] music", 1.0);
        p.line("[install] Tekken 2", 50.0);
        for k in 0..47 {
            p.line(&format!("model_9{k:02}.glb KAZUYA (block {k}): 142 moves from Tekken 2"), 50.0 + k as f64);
        }
        let f = p.fraction(97.0);
        assert!(f > 0.95 && f < 0.98, "{f}");
        assert_eq!(clock(83.4), "1:23");
    }
}
