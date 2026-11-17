//! The game data converted in the browser (web/convert_worker.js runs this in a Web Worker, one thread): the player's
//! files, picked on the page, read by id through functions the worker gives (`okc_*` on its global scope); each part
//! into its pack, two files kept in the site's storage (OPFS, through sync access handles): the Tekken 3 pack
//! (assets.pack, assets.idx: the base, as the page has always kept it), the Tekken 2 and Counter-Strike packs
//! (t2.*, cs.*), the extras (extras.*: the packs installed, kumite_packs) and the arcade stages' pack (arcade.*: not
//! installed). The page reads the arcade pack's `assets/`, and the extras over the Tekken 2 pack's `assets/`, over
//! the base (index.html's `stored`).
//!
//! The stores, by number: 0, 1 the base's pack and index; 2, 3 Tekken 2's; 4, 5 Counter-Strike's; 6, 7 the extras';
//! 8 the scratch file an archive (.zip, .7z, ...) is unpacked into while a job converts (emptied after it); 9, 10
//! the arcade stages' pack and index.

use anyhow::{anyhow, bail, Context, Result};
use kumite_packs::Part;
use ok_converter_common::disc::{main_image, Disc, Folder};
use ok_converter_common::io::{list_paths, path_exists, Ctx, DiscFile, Entry, PackVfs, ReadSeek, Store, Vfs};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::io::{Read, Seek, SeekFrom};
use std::sync::Arc;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    fn okc_input_name(id: u32) -> String;
    fn okc_input_size(id: u32) -> f64;
    /// fills `buf` from `at` of the file
    #[wasm_bindgen(catch)]
    fn okc_input_read(id: u32, at: f64, buf: &mut [u8]) -> Result<(), JsValue>;
    #[wasm_bindgen(catch)]
    fn okc_store_read(store: u32, at: f64, buf: &mut [u8]) -> Result<(), JsValue>;
    #[wasm_bindgen(catch)]
    fn okc_store_write(store: u32, at: f64, data: &[u8]) -> Result<(), JsValue>;
    #[wasm_bindgen(catch)]
    fn okc_store_size(store: u32) -> Result<f64, JsValue>;
    #[wasm_bindgen(catch)]
    fn okc_store_truncate(store: u32, len: f64) -> Result<(), JsValue>;
    #[wasm_bindgen(catch)]
    fn okc_store_flush(store: u32) -> Result<(), JsValue>;
    fn okc_log(line: &str);
    fn okc_panic(message: &str);
}

/// A JavaScript exception as an error (the worker throws "Name: message" strings).
fn js(e: JsValue) -> anyhow::Error {
    anyhow!(e.as_string().unwrap_or_else(|| format!("{e:?}")))
}

/// A picked file.
struct Input(u32);

impl DiscFile for Input {
    fn name(&self) -> String {
        okc_input_name(self.0)
    }
    fn open(&self) -> Result<Box<dyn ReadSeek>> {
        let reader = Reader { id: self.0, pos: 0, size: okc_input_size(self.0) as u64 };
        Ok(Box::new(std::io::BufReader::with_capacity(1 << 16, reader)))
    }
}

struct Reader {
    id: u32,
    pos: u64,
    size: u64,
}

impl Read for Reader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = buf.len().min(self.size.saturating_sub(self.pos) as usize);
        let Some(part) = buf.get_mut(..n).filter(|_| n > 0) else { return Ok(0) };
        okc_input_read(self.id, self.pos as f64, part).map_err(|e| std::io::Error::other(js(e)))?;
        self.pos += n as u64;
        Ok(n)
    }
}

impl Seek for Reader {
    fn seek(&mut self, to: SeekFrom) -> std::io::Result<u64> {
        let pos = match to {
            SeekFrom::Start(p) => Some(p),
            SeekFrom::End(d) => self.size.checked_add_signed(d),
            SeekFrom::Current(d) => self.pos.checked_add_signed(d),
        };
        self.pos = pos.ok_or_else(|| std::io::Error::other("a seek before the start of the file"))?;
        Ok(self.pos)
    }
}

/// The files picked for a disc image (a .cue with its .bin files, a .chd, ...), by name.
struct Picked(BTreeMap<String, u32>);

impl Folder for Picked {
    fn files(&self) -> Vec<(String, u64)> {
        self.0.iter().map(|(n, &id)| (n.clone(), okc_input_size(id) as u64)).collect()
    }
    fn file(&self, name: &str) -> Option<Arc<dyn DiscFile>> {
        self.0.get(name).map(|&id| Arc::new(Input(id)) as Arc<dyn DiscFile>)
    }
}

/// A picked folder (the Half-Life folder): its files by path (below the picked folder), read only.
struct GameFolder(BTreeMap<String, u32>);

impl Vfs for GameFolder {
    fn read(&self, path: &str) -> Result<Vec<u8>> {
        let &id = self.0.get(path).with_context(|| format!("no {path} in the folder"))?;
        let mut buf = vec![0; okc_input_size(id) as usize];
        okc_input_read(id, 0.0, &mut buf).map_err(js).with_context(|| format!("cannot read {path}"))?;
        Ok(buf)
    }
    fn write(&self, path: &str, _: &[u8]) -> Result<()> {
        bail!("cannot write {path}: the folder is read only")
    }
    fn exists(&self, path: &str) -> bool {
        path_exists(&self.0, path)
    }
    fn list(&self, dir: &str) -> Result<Vec<Entry>> {
        Ok(list_paths(&self.0, dir))
    }
    fn remove_all(&self, path: &str) -> Result<()> {
        bail!("cannot remove {path}: the folder is read only")
    }
}

/// A file of a pack (OPFS).
struct Opfs(u32);

impl Store for Opfs {
    fn read_at(&self, off: u64, buf: &mut [u8]) -> Result<()> {
        okc_store_read(self.0, off as f64, buf).map_err(js)
    }
    fn write_at(&self, off: u64, data: &[u8]) -> Result<()> {
        okc_store_write(self.0, off as f64, data).map_err(js)
    }
    fn size(&self) -> Result<u64> {
        Ok(okc_store_size(self.0).map_err(js)? as u64)
    }
    fn truncate(&self, len: u64) -> Result<()> {
        okc_store_truncate(self.0, len as f64).map_err(js)
    }
    fn flush(&self) -> Result<()> {
        okc_store_flush(self.0).map_err(js)
    }
}

/// The store an archive is unpacked into.
const SCRATCH: u32 = 8;

/// A part's pack (the extras: None), as its stores hold it.
fn pack(part: Option<Part>) -> Result<PackVfs> {
    let first = match part {
        Some(Part::T3) => 0,
        Some(Part::T2) => 2,
        Some(Part::Cs) => 4,
        // (store 8 is the scratch file)
        Some(Part::Arcade) => 9,
        None => 6,
    };
    PackVfs::open(Box::new(Opfs(first)), Box::new(Opfs(first + 1)))
}

fn part(name: &str) -> Result<Part> {
    Part::ALL.into_iter().find(|p| p.kind() == name).with_context(|| format!("no part {name}"))
}

#[wasm_bindgen(start)]
fn start() {
    std::panic::set_hook(Box::new(|info| okc_panic(&info.to_string())));
}

/// Runs a job: `options` is JSON, `{"job": "convert", "part": "t3" | "t2" | "cs" | "t3arcade", "files": [[path, id], ...]
/// (the picked files: a disc image's, by name, the image found among them; Counter-Strike: the folder's, by path below
/// it; the arcade ROM set: its .zip alone, or its files by name), "now": "YYYY-MM-DD HH:MM:SS"}`, `{"job": "remove", "part": ...}` or `{"job": "install"}`; then the packs
/// installed again. The state after it ([`state`]'s).
#[wasm_bindgen]
pub fn run(options: &str) -> Result<String, String> {
    job(options).map_err(|e| format!("{e:#}"))
}

/// The game data's state, JSON: `{"t3": manifest | null, "t2": ..., "cs": ..., "t3arcade": ..., "installed": bool, "fingerprint":
/// the game's (the extras' when installed, else the base's)}`.
#[wasm_bindgen]
pub fn state() -> Result<String, String> {
    state_json().map(|v| v.to_string()).map_err(|e| format!("{e:#}"))
}

fn manifests() -> Result<kumite_packs::Manifests> {
    let m = |p: Part| -> Result<Option<Value>> { Ok(p.manifest(&pack(Some(p))?)) };
    Ok(kumite_packs::Manifests { t3: m(Part::T3)?, t2: m(Part::T2)?, cs: m(Part::Cs)?, arcade: m(Part::Arcade)? })
}

fn state_json() -> Result<Value> {
    let m = manifests()?;
    let extras = pack(None)?;
    let installed = m.installed(&extras);
    let manifest: Option<Value> = if m.has_packs() && installed { serde_json::from_slice(&extras.read("manifest.json")?).ok() } else { m.t3.clone() };
    Ok(json!({"t3": m.t3, "t2": m.t2, "cs": m.cs, "t3arcade": m.arcade, "installed": installed,
              "fingerprint": manifest.as_ref().and_then(|v| v["fingerprint"].as_str())}))
}

fn job(options: &str) -> Result<String> {
    let o: Value = serde_json::from_str(options)?;
    let log = |line: &str| okc_log(line);
    match o["job"].as_str().unwrap_or_default() {
        "convert" => {
            let part = part(o["part"].as_str().unwrap_or_default())?;
            let file = |f: &Value| Some((f.get(0)?.as_str()?.to_string(), f.get(1)?.as_u64()? as u32));
            let files: BTreeMap<String, u32> = o["files"].as_array().into_iter().flatten().filter_map(file).collect();
            // a disc image among the files picked (Tekken 3, Tekken 2)
            let disc = || -> Result<Disc> {
                let picked = Picked(files.clone());
                let image =
                    main_image(&picked).context("there is no disc image among the files chosen (.cue, .bin, .chd, .iso, .ccd, .mds, .ecm, .pbp, or a .zip, .7z, .tar, .gz, .xz or .zst of it)")?;
                let scratch = || -> Result<Arc<dyn Store>> { Ok(Arc::new(Opfs(SCRATCH))) };
                let disc = Disc::open_unpacking(&picked, &image, &scratch, &log)?;
                log(&disc.summary());
                Ok(disc)
            };
            let out = pack(Some(part))?;
            let ctx = Ctx { out: &out, log: &log, jobs: 1 };
            let converted = (|| -> Result<()> {
                match part {
                    Part::T3 => {
                        let disc = disc()?;
                        let now = o["now"].as_str().unwrap_or_default();
                        ok_tekken3_converter::convert_all::convert_resumable(&ctx, disc.data()?, disc.audio(2).map(|t| t as &dyn DiscFile), now)?;
                    }
                    Part::T2 => {
                        let disc = disc()?;
                        let cdda = [2, 3].map(|n| disc.audio(n).map(|t| Box::new(t.clone()) as Box<dyn DiscFile>));
                        ok_tekken2_converter::pack::convert(&ctx, disc.data()?, &cdda)?;
                    }
                    Part::Cs => {
                        // (the folder picked is the Half-Life folder, or its cstrike/: then the levels are the game's)
                        let folder = GameFolder(files.clone());
                        let game: Box<dyn Vfs> = if folder.exists("cstrike") || !folder.exists("maps") {
                            Box::new(folder)
                        } else {
                            Box::new(GameFolder(folder.0.into_iter().map(|(p, i)| (format!("cstrike/{p}"), i)).collect()))
                        };
                        ok_hl_cs_converter::pack::convert(&ctx, game.as_ref())?;
                    }
                    // (the arcade ROM set: MAME's .zip alone, else the picked files by name)
                    Part::Arcade => {
                        use ok_tekken3_arcade_converter::roms::{FolderRoms, RomSet, ZipRoms};
                        let folder = GameFolder(files.clone());
                        let zip = files.iter().find(|(name, _)| name.to_ascii_lowercase().ends_with(".zip")).filter(|_| files.len() == 1);
                        let roms: Box<dyn RomSet + '_> = match zip {
                            Some((_, &id)) => Box::new(ZipRoms::new(std::io::BufReader::with_capacity(1 << 16, Reader { id, pos: 0, size: okc_input_size(id) as u64 }))?),
                            None => Box::new(FolderRoms(&folder)),
                        };
                        ok_tekken3_arcade_converter::pack::convert(&ctx, roms.as_ref())?;
                    }
                }
                Ok(())
            })();
            // (the unpacked archive's space given back)
            let _ = Opfs(SCRATCH).truncate(0);
            // (a pack cut off is no pack: emptied; Tekken 3's goes on from its last step next time)
            if converted.is_err() && part != Part::T3 {
                let _ = out.remove_all("").and_then(|()| out.checkpoint(&[]));
            }
            converted?;
        }
        "remove" => {
            let part = part(o["part"].as_str().unwrap_or_default())?;
            let p = pack(Some(part))?;
            p.remove_all("")?;
            p.checkpoint(&[])?;
        }
        "install" => {}
        other => bail!("no job {other:?}"),
    }
    // (the arcade pack is not installed: nothing of the extras changes with it)
    if o["part"] == Part::Arcade.kind() {
        return Ok(state_json()?.to_string());
    }
    let base = pack(Some(Part::T3))?;
    let (t2, cs) = (pack(Some(Part::T2))?, pack(Some(Part::Cs))?);
    let extras = pack(None)?;
    kumite_packs::install(&base, &[(Part::T2, &t2), (Part::Cs, &cs)], &extras, &log)?;
    extras.checkpoint(&[])?;
    Ok(state_json()?.to_string())
}
