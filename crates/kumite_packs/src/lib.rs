//! The game data's packs, over any `Vfs` (natively folders: app/src/packs.rs; in a browser the pack files the page
//! keeps: kumite_convert_web): the Tekken 3 pack is the base; the Tekken 2 and Counter-Strike packs (each from its own
//! copy alone) are installed beside it into the extras, the files they add to or change in the base, with the whole's
//! manifest.json. The game reads the extras over the packs' `assets/` over the base. The Tekken 3 arcade pack is
//! not installed: its stages are files of its own (`assets/arcade/`), outside what the fingerprint covers, so the
//! game data is the same for online play with it or without.

use anyhow::{Context, Result};
use ok_converter_common::io::{Ctx, Layered, Sub, Vfs};
use serde_json::{json, Value};

/// The game data's parts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    /// Tekken 3: the game (required), the base.
    T3,
    /// Tekken 2: its fighters (optional), a pack.
    T2,
    /// Counter-Strike: its levels as arenas (optional), a pack.
    Cs,
    /// Tekken 3, the arcade game: its stages in place of the PlayStation game's (optional), a pack read as it is.
    Arcade,
}

impl Part {
    pub const ALL: [Part; 4] = [Part::T3, Part::T2, Part::Cs, Part::Arcade];
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
        }
    }

    /// The pack's short name (its folder under packs/, its manifest's "pack").
    pub fn kind(self) -> &'static str {
        match self {
            Part::T3 => "t3",
            Part::T2 => ok_tekken2_converter::pack::KIND,
            Part::Cs => ok_hl_cs_converter::pack::KIND,
            Part::Arcade => ok_tekken3_arcade_converter::pack::KIND,
        }
    }

    /// A pack's manifest, checked (the Tekken 3 pack's: of the format this game reads).
    pub fn manifest(self, pack: &dyn Vfs) -> Option<Value> {
        match self {
            Part::T3 => {
                let m: Value = serde_json::from_slice(&pack.read("manifest.json").ok()?).ok()?;
                (m["format"].as_u64()? >= kumite_assets::FORMAT).then_some(m)
            }
            Part::T2 => ok_tekken2_converter::pack::manifest(pack).ok(),
            Part::Cs => ok_hl_cs_converter::pack::manifest(pack).ok(),
            Part::Arcade => ok_tekken3_arcade_converter::pack::manifest(pack).ok(),
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
}

impl Manifests {
    pub fn get(&self, part: Part) -> Option<&Value> {
        match part {
            Part::T3 => self.t3.as_ref(),
            Part::T2 => self.t2.as_ref(),
            Part::Cs => self.cs.as_ref(),
            Part::Arcade => self.arcade.as_ref(),
        }
    }

    /// There are packs to install (the arcade pack is not one).
    pub fn has_packs(&self) -> bool {
        self.t2.is_some() || self.cs.is_some()
    }

    /// What the extras are made of: when it changes they are installed again.
    pub fn stamp(&self) -> Value {
        json!({
            "t3": self.t3.as_ref().map(|m| m["fingerprint"].clone()),
            "t2": self.t2.as_ref().map(|m| json!({"format": m["format"], "source": m["source"]})),
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
            Part::T3 | Part::Arcade => {}
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
            Part::T3 | Part::Arcade => {}
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
