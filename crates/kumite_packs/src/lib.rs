//! The game data's packs, over any `Vfs` (natively folders: app/src/packs.rs; in a browser the pack files the page
//! keeps: kumite_convert_web): the Tekken 3 pack is the base; the Tekken 2 and Counter-Strike packs (each from its own
//! copy alone) are installed beside it into the extras, the files they add to or change in the base, with the whole's
//! manifest.json. The game reads the extras over the packs' `assets/` over the base. The Tekken 3 arcade pack is
//! not installed: its stages are files of its own (`assets/arcade/`), outside what the fingerprint covers, so the
//! game data is the same for online play with it or without. The Tekken Tag Tournament pack is a game of its own
//! (its `assets/` its game root, [`Game::Ttt`]), never installed over Tekken 3; what Tekken 3 brings to it is
//! installed into its own extras ([`install_ttt`]).

use anyhow::{Context, Result};
use ok_converter_common::io::{Ctx, Layered, Sub, Vfs};
use serde_json::{json, Value};

/// The game data's parts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Part {
    /// Tekken 3: the game (required), the base.
    #[default]
    T3,
    /// Tekken 2: its fighters (optional), a pack.
    T2,
    /// Counter-Strike: its levels as arenas (optional), a pack.
    Cs,
    /// Tekken 3, the arcade game: its stages in place of the PlayStation game's (optional), a pack read as it is.
    Arcade,
    /// Tekken Tag Tournament (PlayStation 2): a game of its own (needed when it is the main game), a pack.
    Ttt,
}

impl Part {
    /// (Tekken Tag Tournament last: the parts' indices before it hold)
    pub const ALL: [Part; 5] = [Part::T3, Part::T2, Part::Cs, Part::Arcade, Part::Ttt];
    /// The packs with an `assets/` folder the game reads as it is, the first first.
    pub const LAYERED: [Part; 3] = [Part::T2, Part::Cs, Part::Arcade];
    /// The packs, in their install order (kumite_converter's: Counter-Strike's then Tekken 2's).
    pub const PACKS: [Part; 2] = [Part::Cs, Part::T2];

    pub fn name(self) -> &'static str {
        match self {
            Part::T3 => "Tekken 3",
            Part::T2 => "Tekken 2",
            Part::Cs => "Counter-Strike",
            Part::Arcade => "Tekken 3 arcade",
            Part::Ttt => "Tekken Tag Tournament",
        }
    }

    /// The pack's short name (its folder under packs/, its manifest's "pack").
    pub fn kind(self) -> &'static str {
        match self {
            Part::T3 => "t3",
            Part::T2 => ok_tekken2_converter::pack::KIND,
            Part::Cs => ok_hl_cs_converter::pack::KIND,
            Part::Arcade => ok_tekken3_arcade_converter::pack::KIND,
            Part::Ttt => ok_tekken_tag_converter::pack::KIND,
        }
    }

    /// A pack's manifest: of this part (the Tekken 3 pack's has no "pack"), of any format. An older conversion plays
    /// as it is, marked ([`Part::outdated`]: the Game data screen asks to convert it again), so a converter's new
    /// format never hides a game or its modes.
    pub fn manifest(self, pack: &dyn Vfs) -> Option<Value> {
        let m: Value = serde_json::from_slice(&pack.read("manifest.json").ok()?).ok()?;
        (m["format"].is_i64() && (self == Part::T3 || m["pack"] == self.kind())).then_some(m)
    }

    /// The format its converter writes now.
    pub fn format(self) -> i64 {
        match self {
            Part::T3 => ok_tekken3_converter::convert_all::FORMAT,
            Part::T2 => ok_tekken2_converter::pack::FORMAT,
            Part::Cs => ok_hl_cs_converter::pack::FORMAT,
            Part::Arcade => ok_tekken3_arcade_converter::pack::FORMAT,
            Part::Ttt => ok_tekken_tag_converter::pack::FORMAT,
        }
    }

    /// A manifest of an older conversion than its converter's now (Tekken 3's format 9 brought Gon's fire and gas
    /// pictures): what the new one adds is missing until it is converted again.
    pub fn outdated(self, manifest: &Value) -> bool {
        manifest["format"].as_i64().is_some_and(|f| f < self.format())
    }
}

/// The main game: the one the game plays (the Game data screen's MAIN GAME).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Game {
    #[default]
    T3,
    Ttt,
}

impl Game {
    /// Its part (its pack).
    pub fn part(self) -> Part {
        match self {
            Game::T3 => Part::T3,
            Game::Ttt => Part::Ttt,
        }
    }
}

/// The folder of a pack's files the game reads as they are (a layer of its view, under the extras).
pub const ASSETS: &str = ok_tekken2_converter::pack::ASSETS;

/// The manifests of the parts there are.
#[derive(Clone, Debug, Default)]
pub struct Manifests {
    pub t3: Option<Value>,
    pub t2: Option<Value>,
    pub cs: Option<Value>,
    pub arcade: Option<Value>,
    pub ttt: Option<Value>,
}

impl Manifests {
    pub fn get(&self, part: Part) -> Option<&Value> {
        match part {
            Part::T3 => self.t3.as_ref(),
            Part::T2 => self.t2.as_ref(),
            Part::Cs => self.cs.as_ref(),
            Part::Arcade => self.arcade.as_ref(),
            Part::Ttt => self.ttt.as_ref(),
        }
    }

    /// There are packs to install (the arcade pack is not one).
    pub fn has_packs(&self) -> bool {
        self.t2.is_some() || self.cs.is_some()
    }

    /// What the extras are made of: when it changes they are installed again (Tekken 2's with its installer's version:
    /// an older one's throws named other fighters' victims).
    pub fn stamp(&self) -> Value {
        json!({
            "t3": self.t3.as_ref().map(|m| m["fingerprint"].clone()),
            "t2": self.t2.as_ref().map(|m| json!({"format": m["format"], "source": m["source"], "by": ok_tekken2_converter::pack::INSTALL})),
            "cs": self.cs.as_ref().map(|m| json!({"format": m["format"], "levels": m["levels"]})),
        })
    }

    /// The extras (their manifest.json) are these packs' installed on this base.
    pub fn installed(&self, extras: &dyn Vfs) -> bool {
        if !self.has_packs() {
            return true;
        }
        let m: Option<Value> = extras.read("manifest.json").ok().and_then(|b| serde_json::from_slice(&b).ok());
        self.t3.is_some() && m.is_some_and(|m| m["installed"] == self.stamp())
    }
}

/// Installs the packs (`packs`: each part's pack, in any order; those without a manifest left out) beside the base
/// into `extras`, emptied first, and writes the whole's manifest.json there: the base's, with the packs, what they
/// were installed from and the fingerprint of the view (the extras over the packs' `assets/` over the base). Nothing
/// when there is no base or no pack.
pub fn install(base: &dyn Vfs, packs: &[(Part, &dyn Vfs)], extras: &dyn Vfs, log: &(dyn Fn(&str) + Sync)) -> Result<()> {
    extras.remove_all("")?;
    let mut m = Manifests { t3: Part::T3.manifest(base), ..Manifests::default() };
    for &(part, pack) in packs {
        let man = part.manifest(pack);
        match part {
            Part::T2 => m.t2 = man,
            Part::Cs => m.cs = man,
            Part::T3 | Part::Arcade | Part::Ttt => {}
        }
    }
    let Some(t3) = m.t3.clone() else { return Ok(()) };
    if !m.has_packs() {
        return Ok(());
    }
    let pack = |part: Part| packs.iter().find(|p| p.0 == part && m.get(part).is_some()).map(|p| p.1);
    let assets: Vec<Sub> = Part::PACKS.into_iter().rev().filter_map(pack).filter(|p| p.exists(ASSETS)).map(|p| Sub { inner: p, prefix: ASSETS.into() }).collect();
    let mut below: Vec<&dyn Vfs> = assets.iter().map(|s| s as &dyn Vfs).collect();
    below.push(base);
    let view = Layered { top: extras, below };
    let ctx = Ctx { out: &view, log, jobs: 1 };
    let mut installed = serde_json::Map::new();
    for part in Part::PACKS {
        let (Some(p), Some(man)) = (pack(part), m.get(part)) else { continue };
        // (its converter installs its own format only: an older pack waits, as the Game data screen says, for
        // converting again)
        if man["format"] != part.format() {
            log(&format!("[install] {}: another version's conversion, left out until it is converted again", part.name()));
            continue;
        }
        log(&format!("[install] {}", part.name()));
        match part {
            Part::Cs => {
                ok_hl_cs_converter::pack::install(&ctx, p).context("Counter-Strike")?;
                installed.insert("cs".into(), man["levels"].clone());
            }
            Part::T2 => {
                ok_tekken2_converter::pack::install(&ctx, p).context("Tekken 2")?;
                installed.insert("t2".into(), man["source"].clone());
            }
            Part::T3 | Part::Arcade | Part::Ttt => {}
        }
    }
    // the base's manifest with the packs (as kumite_converter wrote it with extras) and what they were installed from;
    // its fingerprint the whole's
    let mut manifest = t3.as_object().cloned().unwrap_or_default();
    manifest.insert("fingerprint".into(), json!(""));
    manifest.insert("packs".into(), Value::Object(installed));
    manifest.insert("installed".into(), m.stamp());
    log("[install] the fingerprint");
    manifest.insert("fingerprint".into(), json!(ok_tekken3_converter::convert_all::fingerprint(&view)?));
    extras.write("manifest.json", serde_json::to_string_pretty(&Value::Object(manifest))?.as_bytes())?;
    Ok(())
}

/// What Tekken Tag Tournament's extras are made of (its game root's fingerprint, the Tekken 3 pack's and the
/// installer's format): when it changes they are installed again (an older installer's had no Gon's size: his motions'
/// root ran at 100 % and he flew).
pub fn ttt_stamp(game: &dyn Vfs, base: &dyn Vfs) -> Value {
    let game: Option<Value> = game.read("manifest.json").ok().and_then(|b| serde_json::from_slice(&b).ok());
    json!({"ttt": game.map(|m| m["fingerprint"].clone()), "t3": Part::T3.manifest(base).map(|m| m["fingerprint"].clone()), "by": Part::T3.format()})
}

/// The extras (their manifest.json) are Tekken Tag Tournament's installed on these games' data.
pub fn ttt_installed(game: &dyn Vfs, base: &dyn Vfs, extras: &dyn Vfs) -> bool {
    let m: Option<Value> = extras.read("manifest.json").ok().and_then(|b| serde_json::from_slice(&b).ok());
    m.is_some_and(|m| m["installed"] == ttt_stamp(game, base))
}

/// Installs Tekken Tag Tournament's extras (read over its game root `game`) into `extras`, emptied first: what the
/// Tekken 3 pack (`base`) brings to it (Gon and Doctor B.: ok_tekken3_converter's `ttt`), and the whole's
/// manifest.json (the game root's, with what it was installed from, and the fingerprint of the view: the extras over
/// the game root). Nothing without Tekken Tag's game root; the manifest alone (the game root's fingerprint) without
/// the Tekken 3 pack's fighters.
pub fn install_ttt(game: &dyn Vfs, base: &dyn Vfs, extras: &dyn Vfs, log: &(dyn Fn(&str) + Sync)) -> Result<()> {
    extras.remove_all("")?;
    let Some(Value::Object(mut manifest)) = game.read("manifest.json").ok().and_then(|b| serde_json::from_slice(&b).ok()) else { return Ok(()) };
    log(&format!("[install] {}", Part::Ttt.name()));
    if Part::T3.manifest(base).is_some() && base.exists("gamedata/roster.json") {
        let view = Layered { top: extras, below: vec![game] };
        ok_tekken3_converter::ttt::install(&Ctx { out: &view, log, jobs: 1 }, base).context("Tekken 3's fighters in Tekken Tag Tournament")?;
        log("[install] the fingerprint");
        manifest.insert("fingerprint".into(), json!(ok_tekken_tag_converter::assets::fingerprint(&view)?));
    }
    manifest.insert("installed".into(), ttt_stamp(game, base));
    extras.write("manifest.json", serde_json::to_string_pretty(&Value::Object(manifest))?.as_bytes())?;
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // tests: a failure stops them
mod tests {
    use super::*;
    use ok_converter_common::io::StdFs;

    /// A pack of an older format is found and marked out of date; another part's, or a manifest without a format, is
    /// none.
    #[test]
    fn an_older_pack_is_found_and_out_of_date() {
        let dir = std::env::temp_dir().join(format!("ok_packs_older_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let fs = StdFs(dir.clone());
        let found = |part: Part, m: Value| {
            std::fs::write(dir.join("manifest.json"), m.to_string()).unwrap();
            part.manifest(&fs).map(|m| part.outdated(&m))
        };
        assert_eq!(found(Part::T3, json!({"format": Part::T3.format() - 1})), Some(true));
        assert_eq!(found(Part::T3, json!({"format": Part::T3.format()})), Some(false));
        assert_eq!(found(Part::T3, json!({})), None);
        for part in [Part::T2, Part::Cs, Part::Arcade, Part::Ttt] {
            assert_eq!(found(part, json!({"pack": part.kind(), "format": part.format() - 1})), Some(true), "{part:?}");
            assert_eq!(found(part, json!({"pack": part.kind(), "format": part.format()})), Some(false), "{part:?}");
            assert_eq!(found(part, json!({"pack": "other", "format": part.format()})), None, "{part:?}");
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Tekken 3's pack of the format before (#177 took it for none) still brings Gon, Doctor B., Tekken Force and
    /// Tekken Ball to Tekken Tag, as a fresh one does: with OPENKUMITE_ASSETS (a fresh Tekken 3 pack) and
    /// OK_TTT_ASSETS (Tekken Tag's game root).
    #[test]
    fn an_older_tekken_3_pack_still_installs_into_tekken_tag() {
        let (Some(t3), Some(ttt)) = (std::env::var_os("OPENKUMITE_ASSETS"), std::env::var_os("OK_TTT_ASSETS")) else {
            eprintln!("skipped: OPENKUMITE_ASSETS or OK_TTT_ASSETS unset");
            return;
        };
        let (t3, game) = (StdFs(t3.into()), StdFs(ttt.into()));
        let mut m: Value = serde_json::from_slice(&t3.read("manifest.json").unwrap()).unwrap();
        assert!(!Part::T3.outdated(&m), "OPENKUMITE_ASSETS is no fresh Tekken 3 pack");
        let dir = std::env::temp_dir().join(format!("ok_packs_older_t3_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let old = StdFs(dir.join("old"));
        m["format"] = json!(Part::T3.format() - 1);
        old.write("manifest.json", m.to_string().as_bytes()).unwrap();
        for (name, base) in [("older", Layered { top: &old, below: vec![&t3] }), ("fresh", Layered { top: &t3, below: vec![] })] {
            assert_eq!(Part::T3.manifest(&base).map(|m| Part::T3.outdated(&m)), Some(name == "older"));
            let extras = StdFs(dir.join(name));
            install_ttt(&game, &base, &extras, &|_| {}).unwrap();
            assert!(ttt_installed(&game, &base, &extras), "{name}");
            // (an older installer's extras, stamped without its format, are installed again)
            let mut e: Value = serde_json::from_slice(&extras.read("manifest.json").unwrap()).unwrap();
            e["installed"].as_object_mut().unwrap().remove("by");
            let older = StdFs(dir.join(format!("{name}_by")));
            older.write("manifest.json", e.to_string().as_bytes()).unwrap();
            assert!(!ttt_installed(&game, &base, &older), "{name}: an older installer's extras");
            let roster = String::from_utf8(extras.read("gamedata/roster.json").unwrap()).unwrap();
            assert!(roster.contains("Gon") && roster.contains("Doctor B"), "{name}: no Gon or Doctor B.");
            assert!(extras.exists("gamedata/force.json"), "{name}: no Tekken Force");
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
