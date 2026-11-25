//! Loads the converted assets. The engine never touches the disc or original-hardware formats: everything here is
//! standard glTF 2.0 and JSON produced once by kumite_converter.
//!
//! - [`Character`]: skinned mesh, skeleton, every animation (`motion_NNN`, `common_NNN`) as 60 fps tracks.
//! - [`Stage`]: arena geometry flattened into world-space meshes, plus the camera-centred backdrop ("sky")
//!   and, for Force stages, the event nodes (glTF extras).
//!
//! Space: glTF metres, y up (the converters already applied the original hardware → glTF axis change).

pub mod avi;

use anyhow::{bail, Context, Result};
use glam::{Mat4, Quat, Vec3};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// The converted data's layout this engine reads (manifest.json `format`).
pub const FORMAT: u64 = 8;

/// Asset root (`assets/` from convert_all).
#[derive(Clone, Debug)]
pub struct Assets {
    pub root: PathBuf,
}

impl Assets {
    pub fn new(root: impl Into<PathBuf>) -> Result<Self> {
        let root = root.into();
        if !kumite_fs::exists(&root.join("manifest.json")) {
            bail!("{} holds no converted game (no manifest.json): convert your disc into it with kumite_converter", root.display());
        }
        let format = kumite_fs::read(&root.join("manifest.json")).ok().and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok()).and_then(|m| m["format"].as_u64()).unwrap_or(0);
        if format < FORMAT {
            bail!("{} holds an older conversion (format {format}, this OpenKumite reads {FORMAT}): convert your disc into it again with kumite_converter", root.display());
        }
        Ok(Self { root })
    }
    /// The fingerprint of what a fight reads (manifest.json's, from convert_all; empty when it has none).
    pub fn fingerprint(&self) -> String {
        let manifest = kumite_fs::read(&self.root.join("manifest.json")).ok().and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok());
        manifest.and_then(|m| m["fingerprint"].as_str().map(str::to_string)).unwrap_or_default()
    }

    pub fn character(&self, model_record: usize) -> Result<Character> {
        Character::load(&self.root.join(format!("characters/model_{model_record:03}.glb")))
    }
    /// A character glb by file name (`characters/<file>`: the attract embu's own meshes).
    pub fn character_file(&self, file: &str) -> Result<Character> {
        // (a plain file name only: one with a folder or a root in it would read outside `characters/`)
        if std::path::Path::new(file).file_name() != Some(file.as_ref()) {
            bail!("a character file that is not a plain file name: {file:?}");
        }
        Character::load(&self.root.join("characters").join(file))
    }
    pub fn stage(&self, id: usize) -> Result<Stage> {
        Stage::load(&self.root.join(format!("stages/stage_{id:02}.glb")))
    }
    /// The arcade game's stage that stands in for stage `id` (the arcade pack's `arcade/stage_NN.glb`), when there
    /// is one.
    pub fn arcade_stage(&self, id: usize) -> Option<Result<Stage>> {
        let path = self.root.join(format!("arcade/stage_{id:02}.glb"));
        kumite_fs::exists(&path).then(|| Stage::load(&path))
    }
    /// The arcade ROM set's arena that no stage of its game uses (the arcade pack's `arcade/hidden.glb`), when the
    /// pack has it.
    pub fn arcade_hidden(&self) -> Option<Result<Stage>> {
        let path = self.root.join("arcade/hidden.glb");
        kumite_fs::exists(&path).then(|| Stage::load(&path))
    }
    /// A JSON file under `gamedata/`, deserialised into `T`.
    pub fn gamedata<T: serde::de::DeserializeOwned>(&self, name: &str) -> Result<T> {
        let p = self.root.join("gamedata").join(name);
        let b = kumite_fs::read(&p).with_context(|| format!("opening {}", p.display()))?;
        Ok(serde_json::from_slice(&b)?)
    }
}

/// RGBA8 image (alpha 0 = transparent texel, the original hardware's colour-0 rule).
#[derive(Clone, Debug)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SkinVertex {
    pub pos: [f32; 3],
    pub uv: [f32; 2],
    pub joints: [u16; 4],
    pub weights: [f32; 4],
    /// The vertex normal (bind pose; zero when the model has none).
    pub normal: [f32; 3],
    /// Tekken Tag's fighters (TEXCOORD_1, TEXCOORD_2; zero elsewhere): how far its depth lies behind it (metres), its
    /// lighting (0 lit, 1 the texture as it is, 2 environment-mapped, 3 lit with a highlight, 4 the highlight alone),
    /// and the highlight's strength and squarings or the environment map's size in the texture.
    pub look: [f32; 4],
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Vertex {
    pub pos: [f32; 3],
    pub uv: [f32; 2],
    pub color: [f32; 4],
    /// Tekken Tag's floor cell: its centre (x, z), its level of detail (0: every level) and the floor's count of
    /// levels (0: not a floor cell)
    pub cell: [f32; 4],
    /// Tekken Tag's arena: the normal and (w) how its texture coordinates are made from it (material extras `env`:
    /// 1, 2, 4 or 8; 0 as stored)
    pub normal: [f32; 4],
}

/// Tekken Tag's floor cells (0x340070, 0x341218, 0x340408), metres: a cell shows the level of detail of its square's
/// mean depth (the square `half` each way from its centre) past `limits`; laid in a window of `window` cells of
/// `cell` around the fight, cells as far as `reach` from it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FloorLevels {
    pub limits: [f32; 4],
    pub half: [f32; 2],
    pub cell: [f32; 2],
    pub window: Option<[f32; 2]>,
    pub reach: f32,
}

/// A node of the skeleton with its rest transform.
#[derive(Clone, Debug)]
pub struct Node {
    pub name: String,
    pub parent: Option<usize>,
    pub translation: Vec3,
    pub rotation: Quat,
}

/// One animation, sampled once per 60 Hz frame (the converter writes one key per game frame).
#[derive(Clone, Debug, Default)]
pub struct Clip {
    pub name: String,
    pub frames: usize,
    /// Per node: rotation per frame, or `None` if the node keeps its rest rotation.
    pub rotations: Vec<Option<Vec<Quat>>>,
    /// Per node: translation per frame (the root's: root motion; Tekken Tag's scenes' face nodes' places).
    pub translations: Vec<Option<Vec<Vec3>>>,
    /// Per node: scale per frame (Tekken Tag's scenes' face nodes), else none.
    pub scales: Vec<Option<Vec<Vec3>>>,
    /// Tekken Tag: each frame's 57 channels as the game keeps them (animation extras `angles`: the root's heading,
    /// height and distance, then the 18 joint slots' x, y, z, 65536 a turn); empty when the file has none.
    pub angles: Vec<[u16; 57]>,
}

/// gamedata/shadows.json (export_shadows): the fighters' shadow meshes, each with the model records it serves.
#[derive(Clone, Debug, Default, serde::Deserialize)]
pub struct Shadows {
    pub meshes: Vec<ShadowMesh>,
}

/// A shadow mesh: vertices in their joints' local axes (metres), triangles and quads.
#[derive(Clone, Debug, serde::Deserialize)]
pub struct ShadowMesh {
    pub models: Vec<usize>,
    pub joints: Vec<String>,
    /// (joint, x, y, z).
    pub vertices: Vec<(usize, f32, f32, f32)>,
    pub triangles: Vec<[u32; 3]>,
    pub quads: Vec<[u32; 4]>,
}

impl Shadows {
    /// The mesh a model record casts (the Force soldiers: none).
    pub fn of(&self, record: usize) -> Option<&ShadowMesh> {
        self.meshes.iter().find(|m| m.models.contains(&record))
    }
}

/// Tekken Tag's wings (0x3DE4E0): six nodes (one wing's root, middle and tip, then the other's), and each frame of
/// their track their local turn and scale (x, y, z, w, sx, sy, sz).
#[derive(Clone, Debug, serde::Deserialize)]
pub struct Wings {
    pub nodes: [usize; 6],
    pub frames: Vec<[[f32; 7]; 6]>,
}

/// A Tekken Tag fighter's swaying node (a belt, hair, cloth, a tail: the skin's extras `cloth`, from its pack's part
/// 10), as 0x34D258 moves it each drawn frame.
#[derive(Clone, Debug, serde::Deserialize)]
pub struct ClothNode {
    pub node: usize,
    /// How much of its last frame's direction it keeps, and how much gravity pulls it (the rest goes back to its
    /// bind direction).
    pub keep: f32,
    pub gravity: f32,
    /// The frames its tip's place is averaged over.
    pub frames: i32,
    /// Its tip, this far along its x axis (m).
    pub length: f32,
    /// Its turns about y (max, min) and z (max, min) in radians; a pair alike: unlimited.
    pub limits: [f32; 4],
    /// Bit 2: turned about y first, then z.
    pub flags: u8,
}

pub struct Character {
    pub name: String,
    pub nodes: Vec<Node>,
    /// Node order with parents first.
    order: Vec<usize>,
    /// Skin joints (node indices) and their inverse bind matrices.
    pub joints: Vec<usize>,
    pub inverse_bind: Vec<Mat4>,
    pub vertices: Vec<SkinVertex>,
    pub indices: Vec<u32>,
    /// The indices from here on are the blended primitives' (material `BLEND`: Tekken Tag's), drawn after the others
    /// with their texture's alpha; `indices.len()` when there are none.
    pub blend_from: usize,
    /// Where the blended indices' kinds start: the multiplying ones (`shade`, Cd x As: GS ALPHA 0x89) from
    /// `blend_from`, the additive ones (Cs x As + Cd, 0x48) from `layers[0]`, the ordinary ones from `layers[1]`.
    pub layers: [usize; 2],
    pub texture: Image,
    /// The texture's sampler asks for linear filtering (Tekken Tag's fighters: TEX1 0x61, 0x521CE0); else its texels
    /// are sharp.
    pub filtered: bool,
    /// Tekken Tag's Jin: the texels of his texture channel's other frame, the mark (the texture's extras `mark`: the
    /// rectangle x, y, w, h and where in the texture its marked texels are).
    pub mark: Option<[u32; 6]>,
    /// Tekken Tag's True Ogre and Devil: their wings (the skin's extras `wings`).
    pub wings: Option<Wings>,
    pub clips: Vec<Clip>,
    clip_index: HashMap<String, usize>,
    /// Tekken Tag's swaying nodes (none elsewhere).
    pub cloth: Vec<ClothNode>,
    /// Tekken Tag's Unknown's wolf (the skin's extras `follow`): each wolf node and the body node whose turn it takes
    /// (0x34CC38), parents first.
    pub follow: Vec<[usize; 2]>,
    /// Tekken Tag's Mokujin's and Unknown's blades (the skin's extras `moveset`): each node and the moveset it is drawn
    /// with (0x34D108).
    pub blades: Vec<[usize; 2]>,
}

impl Character {
    pub fn load(path: &Path) -> Result<Self> {
        let bytes = kumite_fs::read(path).with_context(|| format!("loading {}", path.display()))?;
        Self::from_glb(&bytes).with_context(|| format!("loading {}", path.display()))
    }

    /// From a glb's bytes (any bytes: a bad file is an error, never a panic).
    pub fn from_glb(bytes: &[u8]) -> Result<Self> {
        unpanic(|| Self::parse_glb(bytes, false))
    }

    /// Add the clips of a motion file (a glb of named joints and their animations, no mesh: Tekken Tag's
    /// `motions/motion_NNNN.glb`), each track on the node of its name as [`Character::with_clips_of`] takes them.
    pub fn add_motions(&mut self, bytes: &[u8]) -> Result<()> {
        let donor = unpanic(|| Self::parse_glb(bytes, true))?;
        for clip in self.clips_of(&donor) {
            self.clip_index.insert(clip.name.clone(), self.clips.len());
            self.clips.push(clip);
        }
        Ok(())
    }

    /// `motions_only`: a motion file, its nodes and clips without a skin, mesh or texture.
    fn parse_glb(bytes: &[u8], motions_only: bool) -> Result<Self> {
        let (doc, buffers, images) = import_glb(bytes)?;
        let get = |b: gltf::Buffer| Some(&buffers.get(b.index())?.0[..]);
        let nodes: Vec<Node> = doc
            .nodes()
            .map(|n| {
                let (t, r, _) = n.transform().decomposed();
                Node { name: n.name().unwrap_or_default().to_string(), parent: None, translation: t.into(), rotation: Quat::from_array(r) }
            })
            .collect();
        let mut nodes = nodes;
        for n in doc.nodes() {
            for c in n.children() {
                nodes[c.index()].parent = Some(n.index());
            }
        }
        let skin = if motions_only { None } else { Some(doc.skins().next().context("no skin")?) };
        let joints: Vec<usize> = skin.iter().flat_map(|s| s.joints().map(|j| j.index())).collect();
        let inverse_bind: Vec<Mat4> = match &skin {
            Some(s) => s.reader(get).read_inverse_bind_matrices().context("no inverse bind matrices")?.map(|m| Mat4::from_cols_array_2d(&m)).collect(),
            None => Vec::new(),
        };
        if inverse_bind.len() != joints.len() {
            bail!("a skin's inverse bind matrices are not one a joint");
        }
        let mut vertices = Vec::new();
        // the opaque primitives, and the blended ones by kind: multiplying (`shade`), `additive`, ordinary
        let (mut indices, mut blended) = (Vec::new(), [Vec::new(), Vec::new(), Vec::new()]);
        let mesh = if motions_only { None } else { Some(doc.meshes().next().context("no mesh")?) };
        for prim in mesh.iter().flat_map(|m| m.primitives()) {
            let r = prim.reader(get);
            let base = vertices.len() as u32;
            let pos: Vec<[f32; 3]> = r.read_positions().context("positions")?.collect();
            let uv: Vec<[f32; 2]> = r.read_tex_coords(0).context("uv")?.into_f32().collect();
            let jo: Vec<[u16; 4]> = r.read_joints(0).context("joints")?.into_u16().collect();
            let we: Vec<[f32; 4]> = r.read_weights(0).context("weights")?.into_f32().collect();
            let nm: Vec<[f32; 3]> = r.read_normals().map(|n| n.collect()).unwrap_or_default();
            let look = |k| -> Vec<[f32; 2]> { r.read_tex_coords(k).map(|t| t.into_f32().collect()).unwrap_or_default() };
            let (l1, l2) = (look(1), look(2));
            if uv.len() < pos.len() || jo.len() < pos.len() || we.len() < pos.len() || jo.iter().flatten().any(|&j| j as usize >= joints.len()) {
                bail!("a primitive's attributes disagree");
            }
            for (i, (((&pos, &uv), &joints), &weights)) in pos.iter().zip(&uv).zip(&jo).zip(&we).enumerate() {
                let ([a, b], [c, d]) = (l1.get(i).copied().unwrap_or_default(), l2.get(i).copied().unwrap_or_default());
                vertices.push(SkinVertex { pos, uv, joints, weights, normal: nm.get(i).copied().unwrap_or_default(), look: [a, b, c, d] });
            }
            let prim_indices: Vec<u32> = r.read_indices().context("indices")?.into_u32().collect();
            if prim_indices.iter().any(|&i| i as usize >= pos.len()) {
                bail!("an index past the vertices");
            }
            let to = match (prim.material().alpha_mode(), prim.material().name()) {
                (gltf::material::AlphaMode::Blend, Some("shade")) => &mut blended[0],
                (gltf::material::AlphaMode::Blend, Some("additive")) => &mut blended[1],
                (gltf::material::AlphaMode::Blend, _) => &mut blended[2],
                _ => &mut indices,
            };
            to.extend(prim_indices.into_iter().map(|i| i + base));
        }
        let blend_from = indices.len();
        let layers = [blend_from + blended[0].len(), blend_from + blended[0].len() + blended[1].len()];
        for b in &mut blended {
            indices.append(b);
        }
        let texture = if motions_only { Image { width: 0, height: 0, rgba: Vec::new() } } else { to_image(images.first().context("no texture")?)? };
        let mut clips = Vec::new();
        for a in doc.animations() {
            let mut clip =
                Clip { name: a.name().unwrap_or_default().to_string(), rotations: vec![None; nodes.len()], translations: vec![None; nodes.len()], scales: vec![None; nodes.len()], ..Clip::default() };
            let extras = a.extras().as_ref().and_then(|x| serde_json::from_str::<serde_json::Value>(x.get()).ok()).unwrap_or_default();
            if let Some(acc) = extras["angles"]
                .as_u64()
                .and_then(|i| doc.accessors().nth(i as usize))
                .filter(|a| a.data_type() == gltf::accessor::DataType::U16 && a.dimensions() == gltf::accessor::Dimensions::Scalar)
            {
                let v: Vec<u16> = gltf::accessor::Iter::<u16>::new(acc, get).map(Iterator::collect).unwrap_or_default();
                clip.angles = v.as_chunks::<57>().0.to_vec();
            }
            for ch in a.channels() {
                let r = ch.reader(get);
                let node = ch.target().node().index();
                match r.read_outputs().context("animation outputs")? {
                    gltf::animation::util::ReadOutputs::Rotations(q) => {
                        let v: Vec<Quat> = q.into_f32().map(|q| Quat::from_array(q).normalize()).collect();
                        clip.frames = clip.frames.max(v.len());
                        clip.rotations[node] = Some(v);
                    }
                    gltf::animation::util::ReadOutputs::Translations(t) => {
                        let v: Vec<Vec3> = t.map(Vec3::from).collect();
                        clip.frames = clip.frames.max(v.len());
                        clip.translations[node] = Some(v);
                    }
                    gltf::animation::util::ReadOutputs::Scales(s) => {
                        let v: Vec<Vec3> = s.map(Vec3::from).collect();
                        clip.frames = clip.frames.max(v.len());
                        clip.scales[node] = Some(v);
                    }
                    _ => {}
                }
            }
            clips.push(clip);
        }
        let clip_index = clips.iter().enumerate().map(|(i, c)| (c.name.clone(), i)).collect();
        let mut order = Vec::new();
        let mut placed = vec![false; nodes.len()];
        while order.len() < nodes.len() {
            let before = order.len();
            for i in 0..nodes.len() {
                if !placed[i] && nodes[i].parent.is_none_or(|p| placed[p]) {
                    placed[i] = true;
                    order.push(i);
                }
            }
            // (import_glb refuses cycles; this only keeps a loop impossible)
            if order.len() == before {
                bail!("the nodes make a cycle");
            }
        }
        let name = doc.scenes().next().and_then(|s| s.name().map(str::to_string)).unwrap_or_default();
        let extras = doc.textures().next().and_then(|t| serde_json::from_str::<serde_json::Value>(t.extras().as_ref()?.get()).ok());
        let mark = extras.and_then(|e| serde_json::from_value(e["mark"].clone()).ok());
        let filtered = doc.textures().next().is_some_and(|t| t.sampler().mag_filter() == Some(gltf::texture::MagFilter::Linear));
        let extras = doc.skins().next().and_then(|s| serde_json::from_str::<serde_json::Value>(s.extras().as_ref()?.get()).ok());
        let wings = extras.as_ref().and_then(|e| serde_json::from_value::<Wings>(e["wings"].clone()).ok()).filter(|w| w.nodes.iter().all(|&n| n < nodes.len()) && !w.frames.is_empty());
        let pairs = |key: &str| -> Vec<[usize; 2]> { extras.as_ref().and_then(|e| serde_json::from_value(e[key].clone()).ok()).unwrap_or_default() };
        let mut follow = pairs("follow");
        follow.retain(|&[n, of]| n < nodes.len() && of < nodes.len() && n != of);
        follow.sort_by_key(|&[n, _]| order.iter().position(|&o| o == n));
        let mut blades = pairs("moveset");
        blades.retain(|&[n, _]| n < nodes.len());
        let cloth: Vec<ClothNode> = extras.and_then(|e| serde_json::from_value(e["cloth"].clone()).ok()).unwrap_or_default();
        let cloth = cloth.into_iter().filter(|c| nodes.get(c.node).is_some_and(|n| n.parent.is_some())).collect();
        Ok(Self { name, nodes, order, joints, inverse_bind, vertices, indices, blend_from, layers, texture, filtered, mark, wings, clips, clip_index, cloth, follow, blades })
    }

    /// The wings at their track's `frame` (clamped to its end): each wing node turned and scaled so in its parent, its
    /// bind position kept, then the world matrices of `world` (a pose's) made again for them.
    pub fn pose_wings(&self, world: &mut [Mat4], frame: usize) {
        let Some(w) = &self.wings else { return };
        let Some(f) = w.frames.get(frame.min(w.frames.len() - 1)) else { return };
        let mut order: Vec<(usize, &[f32; 7])> = w.nodes.iter().copied().zip(f).collect();
        // (parents first: a wing's root comes before its middle and tip)
        order.sort_by_key(|o| o.0);
        for (i, &[x, y, z, q, sx, sy, sz]) in order {
            let n = &self.nodes[i];
            let local = Mat4::from_scale_rotation_translation(Vec3::new(sx, sy, sz), Quat::from_xyzw(x, y, z, q).normalize(), n.translation);
            world[i] = n.parent.and_then(|p| world.get(p)).map_or(local, |p| *p * local);
        }
    }

    /// Unknown's wolf as 0x34CC38 poses it ([`Character::follow`]): each wolf node keeps its own place in its parent
    /// and takes its body node's turn in that one's parent; the root (the body's root's follower) is `root`
    /// (character space).
    pub fn pose_follow(&self, world: &mut [Mat4], root: Mat4) {
        for &[n, of] in &self.follow {
            let (Some(node), Some(&body)) = (self.nodes.get(n), world.get(of)) else { continue };
            let Some(parent) = node.parent.and_then(|p| world.get(p)).copied() else { continue };
            let new = match self.nodes.get(of).and_then(|b| b.parent).and_then(|p| world.get(p)) {
                Some(q) => parent * Mat4::from_rotation_translation(Quat::from_mat4(&(q.inverse() * body)), node.translation),
                None => root,
            };
            world[n] = new;
        }
    }

    /// A shadow mesh skinned to this skeleton: each vertex in the bind pose, wholly on its joint; the quads split
    /// in two. None when a joint is missing (or the mesh's numbers point nowhere).
    pub fn shadow(&self, mesh: &ShadowMesh) -> Option<(Vec<SkinVertex>, Vec<u32>)> {
        let joints: Vec<usize> = mesh.joints.iter().map(|n| self.joints.iter().position(|&j| self.nodes.get(j).is_some_and(|node| node.name == *n))).collect::<Option<_>>()?;
        let vertices = mesh
            .vertices
            .iter()
            .map(|&(j, x, y, z)| {
                let joint = *joints.get(j)?;
                let pos = self.inverse_bind.get(joint)?.inverse().transform_point3(Vec3::new(x, y, z)).to_array();
                Some(SkinVertex { pos, joints: [joint as u16, 0, 0, 0], weights: [1.0, 0.0, 0.0, 0.0], ..Default::default() })
            })
            .collect::<Option<Vec<_>>>()?;
        let n = mesh.vertices.len() as u32;
        if mesh.triangles.iter().flatten().chain(mesh.quads.iter().flatten()).any(|&i| i >= n) {
            return None;
        }
        let mut indices: Vec<u32> = mesh.triangles.iter().flatten().copied().collect();
        indices.extend(mesh.quads.iter().flat_map(|q| [q[0], q[1], q[2], q[1], q[3], q[2]]));
        Some((vertices, indices))
    }

    /// Lit as Tekken Tag's select lights its characters (its VU0 programs, 0x4FA7E0): the highlight's program 0x04A is
    /// a copy of the plain light's (0x000), and the highlight alone (0x0EA) ends at once, its strips not drawn.
    pub fn select_look(&mut self) {
        for v in &mut self.vertices {
            if v.look[1] == 3.0 {
                v.look[1] = 0.0;
            }
        }
        let ends = [self.blend_from, self.layers[0], self.layers[1], self.indices.len()];
        let (mut kept, mut start) = (Vec::with_capacity(self.indices.len()), 0);
        let mut at = [0; 4];
        for (k, &end) in ends.iter().enumerate() {
            let alone = |t: &[u32]| t.iter().all(|&i| self.vertices.get(i as usize).is_some_and(|v| v.look[1] == 4.0));
            kept.extend(self.indices.get(start..end).unwrap_or_default().chunks(3).filter(|t| !alone(t)).flatten());
            (at[k], start) = (kept.len(), end);
        }
        (self.blend_from, self.layers, self.indices) = (at[0], [at[1], at[2]], kept);
    }

    pub fn clip(&self, name: &str) -> Option<usize> {
        self.clip_index.get(name).copied()
    }

    /// World matrix of every node for frame `frame` of clip `clip` (rest pose for nodes without a track).
    /// `lock_root_xz` removes horizontal root motion (the simulation moves the fighter itself).
    pub fn pose(&self, clip: usize, frame: usize, lock_root_xz: bool) -> Vec<Mat4> {
        let c = &self.clips[clip];
        let f = frame.min(c.frames.saturating_sub(1));
        let mut world = vec![Mat4::IDENTITY; self.nodes.len()];
        for &i in &self.order {
            let n = &self.nodes[i];
            let r = c.rotations[i].as_ref().map_or(n.rotation, |v| v[f.min(v.len() - 1)]);
            let mut t = c.translations[i].as_ref().map_or(n.translation, |v| v[f.min(v.len() - 1)]);
            if lock_root_xz && n.parent.is_none() && c.translations[i].is_some() {
                t.x = 0.0;
                t.z = 0.0;
            }
            let local = match c.scales[i].as_ref() {
                Some(s) => Mat4::from_scale_rotation_translation(s[f.min(s.len() - 1)], r, t),
                None => Mat4::from_rotation_translation(r, t),
            };
            world[i] = n.parent.map_or(local, |p| world[p] * local);
        }
        world
    }

    /// Like [`Character::pose`], but playing another character's clip on this skeleton: tracks are matched by
    /// node name (the original's skeletons share their joint names); bones the other character lacks keep their
    /// rest pose, and only the root takes the other clip's translation (bone lengths stay this character's).
    pub fn pose_from(&self, other: &Character, clip: usize, frame: usize, lock_root_xz: bool) -> Vec<Mat4> {
        let c = &other.clips[clip];
        let f = frame.min(c.frames.saturating_sub(1));
        let mut world = vec![Mat4::IDENTITY; self.nodes.len()];
        for &i in &self.order {
            let n = &self.nodes[i];
            let j = other.nodes.iter().position(|o| o.name == n.name);
            let r = j.and_then(|j| c.rotations[j].as_ref()).map_or(n.rotation, |v| v[f.min(v.len() - 1)]);
            let mut t = n.translation;
            if n.parent.is_none() {
                if let Some(v) = j.and_then(|j| c.translations[j].as_ref()) {
                    t = v[f.min(v.len() - 1)];
                    if lock_root_xz {
                        t.x = 0.0;
                        t.z = 0.0;
                    }
                }
            }
            let local = Mat4::from_rotation_translation(r, t);
            world[i] = n.parent.map_or(local, |p| world[p] * local);
        }
        world
    }

    /// This body with `donor`'s clips in place of its own (the copying character): each track goes to the node of
    /// the same name, as [`Character::pose_from`] plays one; only the root takes a translation track, and the
    /// accessory bones (`bone_*`: a stick, hair, wings) keep their rest pose. The root's track is the donor's at
    /// this body's size (a model's size, 90 % to 106 %, is in its bones and its root motion: by the legs' lengths).
    pub fn with_clips_of(&self, donor: &Character) -> Character {
        Character {
            name: self.name.clone(),
            nodes: self.nodes.clone(),
            order: self.order.clone(),
            joints: self.joints.clone(),
            inverse_bind: self.inverse_bind.clone(),
            vertices: self.vertices.clone(),
            indices: self.indices.clone(),
            blend_from: self.blend_from,
            layers: self.layers,
            texture: self.texture.clone(),
            filtered: self.filtered,
            mark: self.mark,
            wings: self.wings.clone(),
            clips: self.clips_of(donor),
            clip_index: donor.clip_index.clone(),
            cloth: self.cloth.clone(),
            follow: self.follow.clone(),
            blades: self.blades.clone(),
        }
    }

    /// `donor`'s clips on this skeleton, as [`Character::with_clips_of`] takes them.
    fn clips_of(&self, donor: &Character) -> Vec<Clip> {
        let from: Vec<Option<usize>> = self.nodes.iter().map(|n| donor.nodes.iter().position(|o| o.name == n.name).filter(|_| !n.name.starts_with("bone_"))).collect();
        let leg = |c: &Character| c.nodes.iter().filter(|n| n.name == "shin_A" || n.name == "foot_A").map(|n| n.translation.length()).sum::<f32>();
        let size = if leg(donor) > 0.0 && leg(self) > 0.0 { leg(self) / leg(donor) } else { 1.0 };
        donor
            .clips
            .iter()
            .map(|c| Clip {
                name: c.name.clone(),
                frames: c.frames,
                rotations: from.iter().map(|j| j.and_then(|j| c.rotations.get(j)?.clone())).collect(),
                translations: self
                    .nodes
                    .iter()
                    .zip(&from)
                    // (the root's scaled to this skeleton; another node's, a scene's face, as it is)
                    .map(|(n, j)| Some(c.translations.get((*j)?)?.as_ref()?.iter().map(|t| if n.parent.is_none() { *t * size } else { *t }).collect()))
                    .collect(),
                scales: from.iter().map(|j| j.and_then(|j| c.scales.get(j)?.clone())).collect(),
                angles: c.angles.clone(),
            })
            .collect()
    }

    /// Skinning matrices (joint world × inverse bind) for a node pose, with a placement transform applied.
    pub fn skin_matrices(&self, node_world: &[Mat4], placement: Mat4) -> Vec<Mat4> {
        self.joints.iter().zip(&self.inverse_bind).map(|(&j, ibm)| placement * node_world[j] * *ibm).collect()
    }
}

/// An arcade stage's sky (0x801A7688 of the arcade program): a panorama of one full turn, scrolled by the camera's
/// yaw (512 columns of the 512 x 480 screen show `fov` degrees) and moved by its pitch: its top row at screen row
/// `-pitch * pitchmul / 256 + ybase` (pitch 4096 a turn, down positive), kept within `min_y..=max_y`; above it the
/// `top` colour, below it (to row 360) the `bottom` colour.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SkyPicture {
    /// Index into [`Stage::textures`].
    pub image: usize,
    pub fov: f32,
    pub ybase: f32,
    pub pitchmul: f32,
    pub top: Option<[u8; 3]>,
    pub bottom: Option<[u8; 3]>,
    pub max_y: Option<f32>,
    pub min_y: Option<f32>,
}

/// A textured mesh in world space (texture = index into [`Stage::textures`]).
#[derive(Clone, Debug, Default)]
pub struct Mesh {
    pub texture: usize,
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
    /// How it moves (the primitive's extras: the arcade stages' carousel, helicopter rotors and flames).
    pub motion: Option<Motion>,
    /// 1 added to what is behind it, 2 multiplying it (its material named `additive` or `shade`: Tekken Tag's
    /// lights and gloss, its painted shadows), 3 blended over it by the texel's and the vertex's alpha; 0 drawn over
    /// it; 4 its texels of full alpha drawn over it, the others blended (Tekken Tag's floor cells).
    pub blend: u8,
}

/// A stage mesh's motion, by frames of 1/60 s.
#[derive(Clone, Debug, PartialEq)]
pub enum Motion {
    /// It turns about the line through `pivot` (m) along `axis`, `rate` radians a frame (extras `spin`); after
    /// `period` frames, where it has one, it is back where it began (a carousel whose model is its near half).
    Spin { pivot: Vec3, axis: Vec3, rate: f32, period: Option<u32> },
    /// Its texture and the `frames - 1` after it follow one another, each shown for `step` frames (extras `frames`,
    /// `step`).
    Frames { frames: usize, step: u32 },
    /// Its vertices are in the axes of the last of these nodes, each placed in the one before (the first in the
    /// stage's): a Tekken Tag arena's object (scene node `objects`) as its arena's code moves it.
    Track(Vec<Joint>),
}

/// A Tekken Tag texture channel over one of a stage's textures: the rectangle (x, y, w, h) its frames change in
/// texture `image`, the textures holding each frame's texels there (`first`, `frames` of them), its speed (256ths of
/// a frame a frame) and first frame ([`channel_frame`]); a still one an arena's task plays (speed 0, resting on
/// `start`) names its `channel`.
#[derive(Clone, Debug, serde::Deserialize)]
pub struct TextureChannel {
    pub image: usize,
    pub rect: [u32; 4],
    pub first: usize,
    pub frames: usize,
    pub speed: u32,
    pub start: u32,
    #[serde(default)]
    pub channel: Option<usize>,
}

/// The picture a Tekken Tag texture channel of `frames` shows on frame `t` (0x3A6C80, state 4): it starts on `start`
/// and goes on `speed` / 256 a frame; past the last it goes back to 0 (the fraction dropped).
pub fn channel_frame(frames: u32, speed: u32, start: u32, t: u64) -> usize {
    let (end, speed, first) = (u64::from(frames.max(1)) << 8, u64::from(speed.max(1)), u64::from(start.min(frames.saturating_sub(1))) << 8);
    // (frames to the end from the start, and of a whole loop)
    let (lead, period) = ((end - first).div_ceil(speed), end.div_ceil(speed));
    let at = if t < lead { first + t * speed } else { (t - lead) % period * speed };
    (at >> 8) as usize
}

/// A stage node: its place in its parent (translation, rotation, scale) and its animation, a key a frame (glTF
/// channels; the converter writes one a frame): after the last key the first comes again, so the last one is only
/// where the motion goes on.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Joint {
    pub rest: (Vec3, Quat, Vec3),
    pub translations: Vec<Vec3>,
    pub rotations: Vec<Quat>,
    /// Turned about y to face the eye, its other turns kept (node extras `"face": "camera"`: Xiaoyu's lamps).
    pub face: bool,
    /// Its node's name (`object_K`: what the engine moves itself, `kumite_game::arena_motion`).
    pub name: String,
}

impl Joint {
    /// Its place in its parent at `frame`.
    pub fn at(&self, frame: u64) -> Mat4 {
        let key = |n: usize| (n > 1).then(|| (frame % (n as u64 - 1)) as usize);
        let t = key(self.translations.len()).and_then(|k| self.translations.get(k)).copied().unwrap_or(self.rest.0);
        let r = key(self.rotations.len()).and_then(|k| self.rotations.get(k)).copied().unwrap_or(self.rest.1);
        Mat4::from_scale_rotation_translation(self.rest.2, r, t)
    }

    /// A chain's place (the first joint's in the stage) at `frame`, under `place`; a joint that faces the eye turned
    /// about y towards `eye` (0x3D5640: its turn the camera's less its parent's).
    pub fn chain(chain: &[Joint], frame: u64, place: Mat4, eye: Vec3) -> Mat4 {
        Self::chain_with(chain, frame, place, eye, &|_| None)
    }

    /// [`Joint::chain`], a joint's place in its parent `over`'s where it names one by the joint's name.
    pub fn chain_with(chain: &[Joint], frame: u64, place: Mat4, eye: Vec3, over: &dyn Fn(&str) -> Option<Mat4>) -> Mat4 {
        chain.iter().fold(place, |m, j| {
            let local = over(&j.name).unwrap_or_else(|| j.at(frame));
            if !j.face {
                return m * local;
            }
            let (s, _, t) = local.to_scale_rotation_translation();
            let (to, ahead) = (eye - (m * local).w_axis.truncate(), m.transform_vector3(Vec3::Z));
            let turn = to.x.atan2(to.z) - ahead.x.atan2(ahead.z);
            m * Mat4::from_scale_rotation_translation(s, Quat::from_rotation_y(turn), t)
        })
    }
}

/// A Force placement (glTF node extras written by export_stages).
#[derive(Clone, Debug)]
pub struct Placement {
    pub name: String,
    pub position: Vec3,
    pub extras: serde_json::Value,
}

pub struct Stage {
    pub name: String,
    pub textures: Vec<Image>,
    /// The backdrop drum (centred on the origin): the game draws it centred on the fighters' midpoint, its base
    /// `sky_base_y` metres below the floor, after the floor and without depth.
    pub sky: Vec<Mesh>,
    /// Depth of the drum's base ring below the floor (metres; the EXE's per-stage table 0x80025138).
    pub sky_base_y: f32,
    /// View-space Y scale of the drum against the world (its 5/3 Y row vs 7122/4096).
    pub sky_y_scale: f32,
    /// The drum is drawn before the floor (on the stage stages.json names), else after it.
    pub sky_first: bool,
    /// The full-screen fill behind everything (sRGB 0..255; 0x800482B0: one stage's sky blue, black elsewhere).
    pub background: [u8; 3],
    /// Repeat period of the floor map (m, x and z) when the floor is laid around the fighters (every arena but
    /// Force): the floor meshes may be shifted by whole periods.
    pub floor_period: Option<[f32; 2]>,
    /// Everything else, flattened to world space (floor copies).
    pub meshes: Vec<Mesh>,
    /// Force: `sky` is the backdrop wall (world space), the original's 2D strip.
    pub backdrop_wall: bool,
    /// Pickup sprite meshes (node-local; place at each item placement).
    pub item_mesh: Option<Mesh>,
    /// Ball's sky: a unit drum carrying the 2D strip, and where the strip goes (game units: its bottom at
    /// the point `bottom_ahead` in front of the camera at height `bottom_y`, y down; `height` px tall).
    pub strip: Option<(Mesh, [f32; 3])>,
    /// An extra stage: its sky is a unit drum centred on the camera (the panorama and its fills),
    /// drawn first without depth.
    pub camera_drum: bool,
    /// The extra stages' floor patch: tile size (m) and tiles a side, laid around the fighters' midpoint snapped to
    /// whole tiles (the engine clips the floor copies to it).
    pub floor_patch: Option<[f32; 2]>,
    /// The extra stages' floor lighting: mode, shade and the fade table (every 32nd entry), when the stage has it.
    pub floor_light: Option<(u32, f32, [f32; 32])>,
    /// Tekken Tag's floor cells (scene extras `floor_levels`): their levels of detail and the window they are laid in.
    pub floor_levels: Option<FloorLevels>,
    /// Tekken Tag's arena fog (scene extras `fog`): FOGCOL (0..1) and the coefficient `line[0] + line[1] x depth` (mm,
    /// 255 none); the fogged strips' vertices have 16 added to their normal's w.
    pub fog: Option<([f32; 3], [f32; 2])>,
    /// Tekken Tag's looping texture channels (scene extras `channels`).
    pub channels: Vec<TextureChannel>,
    pub placements: Vec<Placement>,
    /// Per texture (image): the glb asks for linear filtering (with mipmaps) and tiling both ways, as the
    /// extra levels do; the game's own stages ask for nearest and clamp.
    pub filtered: Vec<bool>,
    /// Per texture (image): the glb asks for linear filtering without mipmaps, tiling both ways (Tekken Tag's
    /// arenas: the PS2's TEX1 MXL 0).
    pub smooth: Vec<bool>,
    /// Per smooth texture (image): the glb clamps it both ways (the GS's CLAMP REGION_CLAMP).
    pub clamped: Vec<bool>,
    /// Per texture (image): the glb asks for sharp texels tiling both ways (the arcade stages' tiles).
    pub tiled: Vec<bool>,
    /// The sky is the stage's own model (scene extras `drum: "model"`: the arcade stages), not a drum of panels:
    /// both sides of its faces are drawn, but with `sky_cull` (`drum_cull`: wound anticlockwise from the front).
    pub sky_model: bool,
    pub sky_cull: bool,
    /// Scene extras `drum_depth: "floor"`: Tekken Tag's arenas, which the PS2 draws with one z-buffer for the model,
    /// the floor and the fighters.
    pub sky_floor_depth: bool,
    /// The 2D panorama behind the model (scene extras `sky`), and how it follows the camera.
    pub sky_picture: Option<SkyPicture>,
    /// An extra level (scene extras `level`): its name.
    pub extra_level: Option<String>,
    /// Its walk grid (scene extras `walls`): origin (m, x / z), cell (m), size (cells), rows (bit set = blocked).
    pub walls: Option<StageWalls>,
    /// The free floor the camera may stand over (scene extras `camera_walls`; the fighters' grid when absent).
    pub camera_walls: Option<StageWalls>,
}

#[derive(Clone, Debug)]
pub struct StageWalls {
    pub origin: [f32; 2],
    pub cell: f32,
    pub size: [usize; 2],
    pub rows: Vec<Vec<u8>>,
}

impl Stage {
    pub fn load(path: &Path) -> Result<Self> {
        let bytes = kumite_fs::read(path).with_context(|| format!("loading {}", path.display()))?;
        Self::from_glb(&bytes).with_context(|| format!("loading {}", path.display()))
    }

    /// From a glb's bytes (any bytes: a bad file is an error, never a panic).
    pub fn from_glb(bytes: &[u8]) -> Result<Self> {
        unpanic(|| Self::parse_glb(bytes))
    }

    fn parse_glb(bytes: &[u8]) -> Result<Self> {
        let (doc, buffers, images) = import_glb(bytes)?;
        let get = |b: gltf::Buffer| Some(&buffers.get(b.index())?.0[..]);
        let textures = images.iter().map(to_image).collect::<Result<Vec<_>>>()?;
        let see_through: Vec<bool> = textures.iter().map(translucent).collect();
        let mesh_data = |m: gltf::Mesh, xf: Mat4| -> Result<Vec<Mesh>> {
            let mut out = Vec::new();
            for prim in m.primitives() {
                let r = prim.reader(get);
                let pos: Vec<[f32; 3]> = r.read_positions().context("positions")?.collect();
                let uv: Vec<[f32; 2]> = r.read_tex_coords(0).context("uv")?.into_f32().collect();
                let col: Vec<[f32; 4]> = match r.read_colors(0) {
                    Some(c) => c.into_rgba_f32().collect(),
                    None => vec![[1.0; 4]; pos.len()],
                };
                if uv.len() < pos.len() || col.len() < pos.len() {
                    bail!("a primitive's attributes disagree");
                }
                let texture = prim.material().pbr_metallic_roughness().base_color_texture().map_or(0, |t| t.texture().source().index());
                // a floor's cells (TEXCOORD_1 the centre's x, z, TEXCOORD_2 the level and the count)
                let cells: Vec<[f32; 4]> = match (r.read_tex_coords(1), r.read_tex_coords(2)) {
                    (Some(c), Some(l)) => c
                        .into_f32()
                        .zip(l.into_f32())
                        .map(|([x, z], [l, n])| {
                            let c = xf.transform_point3(Vec3::new(x, 0.0, z));
                            [c.x, c.z, l, n]
                        })
                        .collect(),
                    _ => vec![[0.0; 4]; pos.len()],
                };
                if cells.len() < pos.len() {
                    bail!("a primitive's attributes disagree");
                }
                // Tekken Tag's arena: the normals of a material whose texture coordinates are made from them (extras `env`)
                let extras = prim.material().extras().as_ref().and_then(|x| serde_json::from_str::<serde_json::Value>(x.get()).ok()).unwrap_or_default();
                let env = extras["env"].as_f64().unwrap_or(0.0) as f32;
                // (and its fogged strips: 16 more)
                let w = env + if extras["fog"].as_f64().is_some_and(|f| f > 0.0) { 16.0 } else { 0.0 };
                // (a shade whose picture's alpha is the GS's, 128 = 1 (extras `gs_alpha`): the blend's As up to ~2,
                // arena 4's sky glow)
                let gs_alpha = if extras["gs_alpha"].as_f64().is_some_and(|f| f > 0.0) { 255.0 / 128.0 } else { 1.0 };
                let normals: Vec<[f32; 4]> = match r.read_normals().filter(|_| env > 0.0) {
                    Some(n) => n.map(|n| [n[0], n[1], n[2], w]).collect(),
                    None => vec![[0.0, 0.0, 0.0, w]; pos.len()],
                };
                if normals.len() < pos.len() {
                    bail!("a primitive's attributes disagree");
                }
                let vertices = pos
                    .iter()
                    .zip(&uv)
                    .zip(&col)
                    .zip(&cells)
                    .zip(&normals)
                    .map(|((((&p, &uv), &[r, g, b, a]), &cell), &normal)| Vertex { pos: xf.transform_point3(Vec3::from(p)).to_array(), uv, color: [r, g, b, a * gs_alpha], cell, normal })
                    .collect();
                let indices: Vec<u32> = r.read_indices().context("indices")?.into_u32().collect();
                if indices.iter().any(|&i| i as usize >= pos.len()) {
                    bail!("an index past the vertices");
                }
                let motion = match prim.extras() {
                    Some(e) => {
                        let v: serde_json::Value = serde_json::from_str(e.get())?;
                        let vec = |a: &serde_json::Value| Some(Vec3::new(a[0].as_f64()? as f32, a[1].as_f64()? as f32, a[2].as_f64()? as f32));
                        let spin = &v["spin"];
                        match (vec(&spin["pivot"]), vec(&spin["axis"]).and_then(Vec3::try_normalize), spin["rate"].as_f64(), v["frames"].as_u64(), v["step"].as_u64()) {
                            (Some(pivot), Some(axis), Some(rate), ..) => {
                                let period = spin["period"].as_u64().and_then(|p| u32::try_from(p).ok()).filter(|p| *p > 0);
                                Some(Motion::Spin { pivot: xf.transform_point3(pivot), axis: xf.transform_vector3(axis), rate: rate as f32, period })
                            }
                            // (the frames are images of the glb)
                            (.., Some(frames), Some(step)) if (1..=64).contains(&frames) && texture + frames as usize <= images.len() => {
                                Some(Motion::Frames { frames: frames as usize, step: (step as u32).max(1) })
                            }
                            _ => None,
                        }
                    }
                    None => None,
                };
                let blend = match prim.material().name() {
                    Some("additive") => 1,
                    Some("shade") => 2,
                    // Tekken Tag's ordinary blend (ALPHA 0x44, unnamed) faded by its vertices' alpha: the hall's
                    // floor's dark edge, a mist
                    None if prim.material().alpha_mode() == gltf::material::AlphaMode::Blend && col.iter().any(|c| c[3] < 1.0) => 3,
                    // ... or by its picture's own: scenery mostly see-through (King's waterfall, Eddy's clouds) is
                    // blended, as the GS blends every 0x44 strip; a cut-out at half would leave its shape solid
                    // shortcut: the rest of the scenery is cut at half; upgrade: as the floor's cells below
                    None if prim.material().alpha_mode() == gltf::material::AlphaMode::Blend && see_through.get(texture) == Some(&true) && cells.iter().all(|c| c[3] == 0.0) => 3,
                    // ... a floor cell's (Eddy's grass): its texels of full alpha drawn with their depth, the others
                    // blended over them (TEST 0x5180B: AFAIL FB_ONLY)
                    None if prim.material().alpha_mode() == gltf::material::AlphaMode::Blend && cells.iter().any(|c| c[3] > 0.0) => 4,
                    _ => 0,
                };
                out.push(Mesh { texture, vertices, indices, motion, blend });
            }
            Ok(out)
        };
        let mut st = Stage {
            name: String::new(),
            textures,
            sky: vec![],
            sky_base_y: 0.0,
            sky_y_scale: 1.0,
            sky_first: false,
            backdrop_wall: false,
            background: [0; 3],
            floor_period: None,
            meshes: vec![],
            item_mesh: None,
            strip: None,
            camera_drum: false,
            floor_patch: None,
            floor_light: None,
            floor_levels: None,
            fog: None,
            channels: vec![],
            placements: vec![],
            filtered: vec![false; images.len()],
            tiled: vec![false; images.len()],
            smooth: vec![false; images.len()],
            clamped: vec![false; images.len()],
            sky_model: false,
            sky_cull: false,
            sky_floor_depth: false,
            sky_picture: None,
            extra_level: None,
            walls: None,
            camera_walls: None,
        };
        for t in doc.textures() {
            let s = t.sampler();
            if s.mag_filter() == Some(gltf::texture::MagFilter::Linear) {
                let mipmaps = s.min_filter() != Some(gltf::texture::MinFilter::Linear);
                if let Some(f) = if mipmaps { &mut st.filtered } else { &mut st.smooth }.get_mut(t.source().index()) {
                    *f = true;
                }
                let clamp = gltf::texture::WrappingMode::ClampToEdge;
                if let (false, true, Some(c)) = (mipmaps, s.wrap_s() == clamp && s.wrap_t() == clamp, st.clamped.get_mut(t.source().index())) {
                    *c = true;
                }
            } else if s.wrap_t() == gltf::texture::WrappingMode::Repeat {
                if let Some(f) = st.tiled.get_mut(t.source().index()) {
                    *f = true;
                }
            }
        }
        let scene = doc.scenes().next().context("no scene")?;
        st.name = scene.name().unwrap_or_default().to_string();
        if let Some(extras) = scene.extras() {
            let v: serde_json::Value = serde_json::from_str(extras.get())?;
            st.sky_base_y = v["drum_base_y"].as_f64().unwrap_or(0.0) as f32 / 1000.0;
            st.sky_y_scale = v["drum_y_scale"].as_f64().unwrap_or(1.0) as f32;
            st.sky_first = v["drum_before_floor"].as_bool().unwrap_or(false);
            st.sky_model = v["drum"] == "model";
            st.sky_cull = v["drum_cull"] == true;
            st.sky_floor_depth = v["drum_depth"] == "floor";
            let sky = &v["sky"];
            if let Some(image) = sky["image"].as_u64().map(|i| i as usize).filter(|&i| i < images.len()) {
                let num = |k: &str| sky[k].as_f64().map(|x| x as f32);
                let rgb = |k: &str| sky[k].as_array().filter(|a| a.len() == 3).map(|a| std::array::from_fn(|i| a[i].as_u64().unwrap_or(0).min(255) as u8));
                st.sky_picture = Some(SkyPicture {
                    image,
                    fov: num("fov").filter(|f| *f >= 1.0).unwrap_or(40.0),
                    ybase: num("ybase").unwrap_or(0.0),
                    pitchmul: num("pitchmul").unwrap_or(288.0),
                    top: rgb("top"),
                    bottom: rgb("bottom"),
                    max_y: num("max_y"),
                    min_y: num("min_y"),
                });
            }
            if let Some(rgb) = v["background_rgb"].as_array() {
                st.background = std::array::from_fn(|k| rgb.get(k).and_then(|c| c.as_u64()).unwrap_or(0) as u8);
            }
            let p = &v["panorama_strip"];
            let strip_at = p.is_object().then(|| [p["bottom_ahead"].as_f64().unwrap_or(6656.0) as f32, p["bottom_y"].as_f64().unwrap_or(256.0) as f32, p["height"].as_f64().unwrap_or(256.0) as f32]);
            st.strip = strip_at.map(|a| (Mesh::default(), a));
            st.floor_period = v["floor_period"].as_array().and_then(|a| Some([a.first()?.as_f64()? as f32, a.get(1)?.as_f64()? as f32]));
            st.camera_drum = v["camera_drum"].as_bool().unwrap_or(false);
            let fl = &v["floor_levels"];
            let num = |x: &serde_json::Value| x.as_f64().map(|v| v as f32);
            st.floor_levels = (|| {
                let limits = [num(&fl["limits"][0])?, num(&fl["limits"][1])?, num(&fl["limits"][2])?, num(&fl["limits"][3])?];
                let cell = [num(&fl["cell"][0])?, num(&fl["cell"][1])?];
                let window = (|| Some([num(&fl["window"][0])?, num(&fl["window"][1])?]))().filter(|w| w.iter().all(|n| (1.0..=64.0).contains(n)));
                let corner = num(&fl["corner"])?;
                Some(FloorLevels { limits, half: cell.map(|c| c * corner / 2.0), cell, window, reach: num(&fl["reach"]).unwrap_or(f32::MAX) })
            })()
            .filter(|l| l.cell.iter().all(|c| *c > 0.0));
            let fog = &v["fog"];
            st.fog = (|| Some(([0, 1, 2].map(|k| fog["colour"][k].as_f64().unwrap_or(0.0) as f32 / 255.0), [fog["line"][0].as_f64()? as f32, fog["line"][1].as_f64()? as f32])))();
            // (each channel's textures among the stage's, its rectangle inside its image)
            let fits = |c: &TextureChannel| {
                let (Some(im), Some(last)) = (st.textures.get(c.image), c.first.checked_add(c.frames).filter(|e| *e <= st.textures.len())) else { return false };
                let [x, y, w, h] = c.rect.map(u64::from);
                c.frames > 0 && x + w <= u64::from(im.width) && y + h <= u64::from(im.height) && st.textures[c.first..last].iter().all(|t| (u64::from(t.width), u64::from(t.height)) == (w, h))
            };
            st.channels = serde_json::from_value::<Vec<TextureChannel>>(v["channels"].clone()).unwrap_or_default().into_iter().filter(fits).collect();
            st.floor_patch = v["floor_patch"].as_array().and_then(|a| Some([a.first()?.as_f64()? as f32, a.get(1)?.as_f64()? as f32]));
            st.extra_level = v["level"].as_str().map(str::to_string);
            let grid = |w: &serde_json::Value| {
                let f = |x: &serde_json::Value| x.as_f64().unwrap_or(0.0) as f32;
                let hex = |r: &str| (0..r.len() / 2).filter_map(|i| u8::from_str_radix(r.get(2 * i..2 * i + 2)?, 16).ok()).collect();
                w.is_object().then(|| StageWalls {
                    origin: [f(&w["origin"][0]), f(&w["origin"][1])],
                    cell: f(&w["cell"]),
                    size: [w["size"][0].as_u64().unwrap_or(0) as usize, w["size"][1].as_u64().unwrap_or(0) as usize],
                    rows: w["rows"].as_array().into_iter().flatten().map(|r| hex(r.as_str().unwrap_or_default())).collect(),
                })
            };
            st.walls = grid(&v["walls"]);
            st.camera_walls = grid(&v["camera_walls"]).or_else(|| st.walls.clone());
            let fl = &v["floor_light"];
            if fl.is_object() {
                let mut table = [0.0f32; 32];
                for (k, t) in fl["table"].as_array().into_iter().flatten().take(32).enumerate() {
                    table[k] = t.as_f64().unwrap_or(0.0) as f32;
                }
                st.floor_light = Some((fl["mode"].as_u64().unwrap_or(0) as u32, fl["shade"].as_f64().unwrap_or(0.0) as f32, table));
            }
        }
        // the objects' animations: each node's keys
        let mut keys: HashMap<usize, (Vec<Vec3>, Vec<Quat>)> = HashMap::new();
        for ch in doc.animations().flat_map(|a| a.channels()) {
            let k = keys.entry(ch.target().node().index()).or_default();
            match ch.reader(get).read_outputs().context("animation outputs")? {
                gltf::animation::util::ReadOutputs::Translations(t) => k.0 = t.map(Vec3::from).collect(),
                gltf::animation::util::ReadOutputs::Rotations(q) => k.1 = q.into_f32().map(|q| Quat::from_array(q).normalize()).collect(),
                _ => {}
            }
        }
        // (a node under `objects` carries its chain of joints)
        let mut stack: Vec<(gltf::Node, Mat4, Option<Vec<Joint>>)> = scene.nodes().map(|n| (n, Mat4::IDENTITY, None)).collect();
        while let Some((n, parent, chain)) = stack.pop() {
            let xf = parent * Mat4::from_cols_array_2d(&n.transform().matrix());
            let name = n.name().unwrap_or_default();
            if let Some(mut chain) = chain {
                let (t, r, s) = n.transform().decomposed();
                let (translations, rotations) = keys.remove(&n.index()).unwrap_or_default();
                let face = n.extras().as_ref().and_then(|x| serde_json::from_str::<serde_json::Value>(x.get()).ok()).is_some_and(|x| x["face"] == "camera");
                chain.push(Joint { rest: (Vec3::from(t), Quat::from_array(r).normalize(), Vec3::from(s)), translations, rotations, face, name: name.to_string() });
                if let Some(m) = n.mesh() {
                    st.sky.extend(mesh_data(m, Mat4::IDENTITY)?.into_iter().map(|mesh| Mesh { motion: Some(Motion::Track(chain.clone())), ..mesh }));
                }
                stack.extend(n.children().map(|c| (c, xf, Some(chain.clone()))));
                continue;
            }
            if name == "objects" {
                stack.extend(n.children().map(|c| (c, xf, Some(vec![]))));
                continue;
            }
            if let Some(extras) = n.extras() {
                let v: serde_json::Value = serde_json::from_str(extras.get())?;
                st.placements.push(Placement { name: name.to_string(), position: xf.w_axis.truncate(), extras: v });
                if let (Some(m), None) = (n.mesh(), &st.item_mesh) {
                    st.item_mesh = mesh_data(m, Mat4::IDENTITY)?.into_iter().next();
                }
            } else if let Some(m) = n.mesh() {
                if name == "sky" {
                    st.sky.extend(mesh_data(m, Mat4::IDENTITY)?);
                } else if name == "backdrop_wall" {
                    st.sky.extend(mesh_data(m, xf)?);
                    st.backdrop_wall = true;
                } else if name == "panorama_strip" {
                    if let (Some(mesh), Some(strip)) = (mesh_data(m, Mat4::IDENTITY)?.into_iter().next(), st.strip.as_mut()) {
                        strip.0 = mesh;
                    }
                } else {
                    st.meshes.extend(mesh_data(m, xf)?);
                }
            }
            stack.extend(n.children().map(|c| (c, xf, None)));
        }
        Ok(st)
    }
}

/// `gltf::import_slice`, but first the checks its validation lacks (gltf 1.4.1 panics, or asks for all the memory, on
/// such files: found by fuzzing, `fuzz/`, and review): a primitive's attribute naming no accessor, a buffer view
/// outside its buffer, a value out of range, an image not a PNG of the glb's own or too big, a sparse accessor or one
/// without data or outside its view (or strided tighter than its elements), an accessor of a kind its use cannot
/// take (gltf's readers have `unreachable!` there), an animation or skin naming no node, nodes not a tree. Only what
/// the converter writes passes.
fn import_glb(bytes: &[u8]) -> Result<(gltf::Document, Vec<gltf::buffer::Data>, Vec<gltf::image::Data>)> {
    let raw = gltf::Gltf::from_slice_without_validation(bytes)?;
    let json = raw.document.as_json();
    for m in &json.meshes {
        for p in &m.primitives {
            if p.attributes.values().any(|a| a.value() >= json.accessors.len()) {
                bail!("a mesh's attribute names no accessor");
            }
        }
    }
    for v in &json.buffer_views {
        let end = v.byte_offset.map_or(0, |o| o.0 as usize).checked_add(v.byte_length.0 as usize);
        let fits = json.buffers.get(v.buffer.value()).is_some_and(|b| end.is_some_and(|e| e <= b.byte_length.0 as usize));
        if !fits {
            bail!("a buffer view outside its buffer");
        }
    }
    // (a value the parser could not read, kept as `Invalid` and unwrapped by gltf later, does not serialise)
    if serde_json::to_writer(std::io::sink(), json).is_err() {
        bail!("a glb with values out of range");
    }
    // the images: PNGs in the glb's own data (as the converter writes them), of a size worth decoding
    let blob = raw.blob.as_deref().unwrap_or_default();
    let mut pixels = 0u64;
    for i in &json.images {
        let png = i.buffer_view.as_ref().and_then(|v| json.buffer_views.get(v.value())).filter(|v| v.buffer.value() == 0 && json.buffers.first().is_some_and(|b| b.uri.is_none()));
        let png = png.and_then(|v| {
            let at = v.byte_offset.map_or(0, |o| o.0 as usize);
            blob.get(at..at.checked_add(v.byte_length.0 as usize)?)
        });
        let size = png.filter(|_| i.mime_type.as_ref().is_some_and(|m| m.0 == "image/png")).and_then(png_size);
        if !size.is_some_and(|(w, h)| w <= MAX_TEXTURE && h <= MAX_TEXTURE) {
            bail!("an image that is not a PNG of the glb's own, or too big");
        }
        pixels += size.map_or(0, |(w, h)| u64::from(w) * u64::from(h));
    }
    if pixels > MAX_PIXELS {
        bail!("images of more pixels together than a glb has");
    }
    // (a character's clips keep a track per node)
    if json.animations.len().saturating_mul(json.nodes.len()) > MAX_ELEMENTS {
        bail!("more animations times nodes than a glb has");
    }
    // the accessors: plain (no sparse ones), with data, each element whole inside its view
    for a in &json.accessors {
        let (Some(view), true) = (a.buffer_view.as_ref().and_then(|v| json.buffer_views.get(v.value())), a.sparse.is_none()) else {
            bail!("an accessor without its own data");
        };
        let (gltf::json::validation::Checked::Valid(c), gltf::json::validation::Checked::Valid(t)) = (&a.component_type, &a.type_) else {
            bail!("an accessor of no type");
        };
        let element = c.0.size() * t.multiplicity();
        let stride = view.byte_stride.map_or(element, |s| s.0);
        let count = a.count.0 as usize;
        let end = (count.checked_sub(1)).and_then(|n| n.checked_mul(stride)).and_then(|n| n.checked_add(element)).and_then(|n| n.checked_add(a.byte_offset.map_or(0, |o| o.0 as usize)));
        if stride < element || count > MAX_ELEMENTS || !end.is_some_and(|e| e <= view.byte_length.0 as usize) {
            bail!("an accessor outside its view");
        }
    }
    // what reading it takes: a mesh as often as nodes place it, a sampler's output as often as channels use it (a small
    // file using one accessor again and again would otherwise ask for many times its size)
    let count = |i: usize| json.accessors.get(i).map_or(0, |a| a.count.0 as usize);
    let mut uses = vec![0usize; json.meshes.len()];
    for m in json.nodes.iter().filter_map(|n| n.mesh.as_ref()) {
        if let Some(u) = uses.get_mut(m.value()) {
            *u += 1;
        }
    }
    let meshes = json.meshes.iter().zip(&uses).map(|(m, &u)| {
        let size = m.primitives.iter().map(|p| p.attributes.values().map(|a| count(a.value())).sum::<usize>() + p.indices.as_ref().map_or(0, |i| count(i.value()))).sum::<usize>();
        size.saturating_mul(u.max(1))
    });
    let clips = json.animations.iter().flat_map(|a| a.channels.iter().filter_map(|c| a.samplers.get(c.sampler.value()))).map(|s| count(s.output.value()));
    if meshes.chain(clips).fold(0usize, usize::saturating_add) > MAX_READ {
        bail!("more to read than a glb has");
    }
    // each accessor of the kind gltf's readers take for its use (another is an `unreachable!` in them)
    {
        use gltf::json::accessor::{ComponentType as C, Type as T};
        use gltf::json::validation::Checked::Valid;
        let kind = |i: usize| match json.accessors.get(i) {
            Some(gltf::json::Accessor { component_type: Valid(c), type_: Valid(t), .. }) => Some((c.0, *t)),
            _ => None,
        };
        let is = |i: usize, ok: &dyn Fn(C, T) -> bool| kind(i).is_some_and(|(c, t)| ok(c, t));
        let float_or_norm = |c: C| matches!(c, C::F32 | C::U8 | C::U16);
        for p in json.meshes.iter().flat_map(|m| &m.primitives) {
            use gltf::json::mesh::Semantic as S;
            if p.targets.is_some() || !p.indices.as_ref().is_none_or(|i| is(i.value(), &|c, t| matches!(c, C::U8 | C::U16 | C::U32) && t == T::Scalar)) {
                bail!("a primitive's indices of no index type (or morph targets)");
            }
            for (semantic, a) in &p.attributes {
                let ok = match semantic {
                    Valid(S::Positions | S::Normals) => is(a.value(), &|c, t| c == C::F32 && t == T::Vec3),
                    Valid(S::Tangents) => is(a.value(), &|c, t| c == C::F32 && t == T::Vec4),
                    Valid(S::TexCoords(_)) => is(a.value(), &|c, t| float_or_norm(c) && t == T::Vec2),
                    Valid(S::Colors(_)) => is(a.value(), &|c, t| float_or_norm(c) && matches!(t, T::Vec3 | T::Vec4)),
                    Valid(S::Joints(_)) => is(a.value(), &|c, t| matches!(c, C::U8 | C::U16) && t == T::Vec4),
                    Valid(S::Weights(_)) => is(a.value(), &|c, t| float_or_norm(c) && t == T::Vec4),
                    _ => false,
                };
                if !ok {
                    bail!("a primitive's attribute of the wrong kind");
                }
            }
        }
        for an in &json.animations {
            for ch in &an.channels {
                use gltf::json::animation::Property as P;
                let Some(sampler) = an.samplers.get(ch.sampler.value()) else { bail!("an animation channel with no sampler") };
                let output = match ch.target.path {
                    Valid(P::Translation | P::Scale) => is(sampler.output.value(), &|c, t| c == C::F32 && t == T::Vec3),
                    Valid(P::Rotation) => is(sampler.output.value(), &|c, t| c != C::U32 && t == T::Vec4),
                    _ => false,
                };
                if ch.target.node.value() >= json.nodes.len() || !output || !is(sampler.input.value(), &|c, t| c == C::F32 && t == T::Scalar) {
                    bail!("an animation channel of the wrong kind, or of no node");
                }
            }
        }
        for sk in &json.skins {
            let ibm = sk.inverse_bind_matrices.as_ref().is_none_or(|i| is(i.value(), &|c, t| c == C::F32 && t == T::Mat4));
            if !ibm || sk.joints.iter().chain(&sk.skeleton).any(|j| j.value() >= json.nodes.len()) {
                bail!("a skin of the wrong kind, or of no node");
            }
        }
    }
    // the nodes: a tree (one parent at most, no cycle)
    let mut parent = vec![None; json.nodes.len()];
    for (i, n) in json.nodes.iter().enumerate() {
        for c in n.children.iter().flatten() {
            match parent.get_mut(c.value()) {
                Some(p @ None) => *p = Some(i),
                _ => bail!("a node with two parents"),
            }
        }
    }
    for i in 0..parent.len() {
        let (mut at, mut steps) = (i, 0);
        while let Some(p) = parent[at] {
            (at, steps) = (p, steps + 1);
            if steps > parent.len() {
                bail!("the nodes make a cycle");
            }
        }
    }
    Ok(gltf::import_slice(bytes)?)
}

/// A glb's largest texture side (pixels: an arcade stage's sky is one full turn, up to 5120 wide), its images' pixels
/// together (the largest converted ~4M), its largest accessor (elements) and animations times nodes.
const MAX_TEXTURE: u32 = 8192;
const MAX_PIXELS: u64 = 1 << 24;
const MAX_ELEMENTS: usize = 1 << 22;
/// The elements a glb's meshes and clips read at most, all told (the converter's largest: about 2.4 million).
const MAX_READ: usize = 2 * MAX_ELEMENTS;

/// A PNG's width and height (its header's IHDR).
fn png_size(b: &[u8]) -> Option<(u32, u32)> {
    if b.get(..8)? != b"\x89PNG\r\n\x1a\n" || b.get(12..16)? != b"IHDR" {
        return None;
    }
    let be = |o: usize| Some(u32::from_be_bytes(b.get(o..o + 4)?.try_into().ok()?));
    Some((be(16)?, be(20)?))
}

/// Natively a panic still left in gltf on a bad file is an error; on the web (no unwinding) the checks of
/// [`import_glb`] are what keep it from one.
fn unpanic<T>(f: impl FnOnce() -> Result<T>) -> Result<T> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).unwrap_or_else(|_| bail!("a bad glb"))
}

/// A picture most of whose seen texels (alpha over 16) are partly see-through (alpha under 240).
fn translucent(t: &Image) -> bool {
    let seen = t.rgba.as_chunks::<4>().0.iter().filter(|p| p[3] > 16);
    let (all, part) = seen.fold((0usize, 0usize), |(n, k), p| (n + 1, k + usize::from(p[3] < 240)));
    part * 2 > all
}

fn to_image(d: &gltf::image::Data) -> Result<Image> {
    use gltf::image::Format;
    let rgba = match d.format {
        Format::R8G8B8A8 => d.pixels.clone(),
        Format::R8G8B8 => d.pixels.chunks(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect(),
        f => bail!("unsupported texture format {f:?}"),
    };
    Ok(Image { width: d.width, height: d.height, rgba })
}

#[cfg(test)]
mod tests {
    use super::channel_frame;

    #[test]
    fn a_texture_channel_loops_from_its_start() {
        // 4 pictures at 0.75 a frame from picture 2: 2, 2, 3, then from 0 (the fraction dropped): 0, 0, 1, 2, 3, 3
        let shown: Vec<usize> = (0..9).map(|t| channel_frame(4, 0xC0, 2, t)).collect();
        assert_eq!(shown, [2, 2, 3, 0, 0, 1, 2, 3, 3]);
        assert_eq!((0..5).map(|t| channel_frame(2, 0x100, 0, t)).collect::<Vec<_>>(), [0, 1, 0, 1, 0]);
    }
}
