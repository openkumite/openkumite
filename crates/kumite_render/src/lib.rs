//! wgpu renderer for OpenKumite.
//!
//! Everything is in the converted assets' space: glTF metres, right-handed, y up.
//!
//! Draw kinds:
//! - [`SkinnedModel`]: characters, standard 4-joint linear blend skinning (the converter turned the engine's
//!   seam welding into skin weights). Skinning matrices are uploaded per frame with [`Renderer::update_bones`].
//! - [`StaticMesh`]: stage floor and props (unlit, texture × vertex colour, like the original hardware stage code).
//! - Sky meshes: the backdrop drum, drawn first, rotation-only, centred on the camera, without depth.
//! - Debug lines ([`LineVertex`]): drawn last, on top of everything.
//!
//! An [`Overlay`] (the debug UI) can draw into the frame after the scene, before it is presented.

use anyhow::{Context, Result};
use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};
use std::sync::Arc;
use wgpu::util::DeviceExt;

pub use wgpu;

/// Debug line vertex: world position (metres), RGBA colour. Pairs of vertices form lines.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct LineVertex {
    pub pos: [f32; 3],
    pub color: [f32; 4],
}

/// Something drawn over the finished scene (e.g. a debug UI), into the same frame.
pub trait Overlay {
    /// 2D drawn behind the characters: after the sky and the stage, before the fighters (the original hardware game's
    /// HUD list B, linked behind the 3D characters). It goes into the 3D scene's target: `scale` is that target's size
    /// over the window's (the render scale).
    fn draw_under(&mut self, _device: &wgpu::Device, _queue: &wgpu::Queue, _encoder: &mut wgpu::CommandEncoder, _target: &wgpu::TextureView, _size: [u32; 2], _scale: f32) {}
    fn draw(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, encoder: &mut wgpu::CommandEncoder, target: &wgpu::TextureView, size: [u32; 2]);
}

/// Per-frame numbers for the debug UI.
#[derive(Clone, Copy, Debug, Default)]
pub struct RenderStats {
    /// The frame was not drawn (window hidden / surface lost).
    pub skipped: u32,
    pub draw_calls: u32,
    pub triangles: u32,
    pub lines: u32,
}

/// Effect particles for one frame: camera-facing (or floor) quads as triangles, textured from an atlas whose
/// alpha is the sprite's brightness. `alpha` quads are blended over, `additive` ones add light.
pub struct Particles<'a> {
    pub texture: &'a Texture,
    pub alpha: &'a [StaticVertex],
    pub additive: &'a [StaticVertex],
    /// Untextured triangles in their vertex colours, blended over / added (drawn first, in the order given:
    /// sort them back to front).
    pub flat_alpha: &'a [StaticVertex],
    pub flat_add: &'a [StaticVertex],
    /// Textured quads as the PS2's GS draws them (Tekken Tag's arenas' particles): blended by the texture's alpha
    /// times the colour's / adding the texture times the colour, nothing cut away (drawn last).
    pub soft_alpha: &'a [StaticVertex],
    pub soft_add: &'a [StaticVertex],
}

/// A captured frame: tightly packed RGBA8.
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Vertex of a skinned character mesh (see the crate docs).
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct SkinVertex {
    pub pos: [f32; 3],
    pub uv: [f32; 2],
    pub joints: [u16; 4],
    pub weights: [f32; 4],
    /// Bind-pose normal (zero: lit by the face's own normal).
    pub normal: [f32; 3],
}

/// The fighters' shadows (0x8003AFCC): each fighter's shadow mesh moved along the light onto the floor, drawn in
/// one opaque colour after the stage, before the fighters; or (Tekken Tag's, 0x340E28) the floor under it darkened.
#[derive(Clone, Copy, Debug)]
pub struct Shadow {
    /// Towards the light (metres' axes, y up): the shadow falls the other way.
    pub toward: Vec3,
    /// The floor's height (metres).
    pub floor: f32,
    /// sRGB.
    pub colour: Vec3,
    /// Tekken Tag's: the floor darkened by this much (0-1, of its displayed colour) once under each fighter's
    /// shadow, however its triangles overlap (the game draws the silhouette into a picture first); None: opaque.
    pub alpha: Option<f32>,
}

/// A stage's light for the fighters (the original's per-stage table 0x80096C78, NCCS): the ambient, the key light's
/// colour and the direction towards it (metres' axes), the material colour (128 = 1); a fill straight from above at
/// half the key colour.
#[derive(Clone, Copy, Debug)]
pub struct StageLight {
    pub ambient: Vec3,
    pub key: Vec3,
    pub toward: Vec3,
    pub material: Vec3,
    /// Tekken Tag's: its two other lights (colour, towards each), and no fill from above.
    pub more: Option<[(Vec3, Vec3); 2]>,
}

/// Vertex of a static (stage) mesh: world position, atlas UV, RGBA modulation colour.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct StaticVertex {
    pub pos: [f32; 3],
    pub uv: [f32; 2],
    pub color: [f32; 4],
    /// Tekken Tag's floor cell: its centre (x, z, metres, moved with the floor), its level of detail (0: every
    /// level) and the floor's count of levels; zeros elsewhere
    pub cell: [f32; 4],
    /// Tekken Tag's arena: the normal and (w) how the microcode makes the texture coordinates from it (shader.wgsl
    /// `env_uv`; 0: as stored)
    pub normal: [f32; 4],
}

/// Camera in metre space (y up), like the web viewer.
#[derive(Clone, Copy, Debug)]
pub struct Camera {
    pub eye: Vec3,
    pub target: Vec3,
    pub fov_y: f32,
    /// The picture stretched up and down by so much (Tekken Tag: the PS2 projects as square pixels onto its 640 x 448
    /// screen, which a 4:3 television shows 480 / 448 as high).
    pub y_stretch: f32,
    /// The picture turned anticlockwise about the line of sight (radians).
    pub roll: f32,
    /// The backdrop drum's placement (world, metres) and its view-space Y scale.
    pub sky: Mat4,
    pub sky_y_scale: f32,
    /// Draw the drum before the stage meshes (the stage stages.json names) instead of after them.
    pub sky_first: bool,
    /// The drum's panels seen from behind are not drawn (the original hardware's NCLIP: one of the original's drums,
    /// which the camera can stand outside of); off for a drum around the camera (an extra stage's, Ball's strip).
    pub sky_cull: bool,
    /// The drum is a model drawn with a depth buffer of its own (an arcade stage's): over the floor everywhere, its
    /// faces hiding one another by their depth, the fighters over all of it. The arcade game sorts its faces instead,
    /// and so do the original's drums here; sorted by triangle, a model's large faces come out in the wrong order.
    pub sky_depth: bool,
    /// With `sky_depth`: the model is depth-tested against the floor too, one depth buffer for all (Tekken Tag's
    /// arenas): the floor hides the ground under it, the model the fighters behind it.
    pub sky_floor_depth: bool,
    /// The stage's fill behind everything (sRGB 0..255), else the renderer's clear colour.
    pub background: Option<[u8; 3]>,
    /// The stage meshes' placement (the floor follows the fighters by whole map periods).
    pub floor: Mat4,
    /// The hit light on the fighters: colour × strength and its position (metres).
    pub hit_light: Option<([f32; 3], Vec3)>,
    /// Tekken Tag's floor cells: their levels of detail and their window.
    pub floor_levels: Option<FloorLevels>,
    /// Tekken Tag's arena fog: FOGCOL (sRGB 0..1) and the coefficient's line (255 + slope x depth in mm).
    pub fog: Option<([f32; 3], [f32; 2])>,
    /// Eddy's grass: the floor cells' vertices moved by a lattice ([`Grass`]).
    pub grass: Option<Grass>,
    /// Tekken Tag's: the 3D pass drawn on the frame's sRGB bytes (textures, colours, blending and fog as the PS2's GS
    /// has them), where the frame has a plain view; else in its linear colour.
    pub gamma: bool,
    /// The stage meshes cut to x0..x1, z0..z1 (metres): the original's finite floor patch, where it shows.
    pub floor_clip: Option<[f32; 4]>,
    /// The extra stages' floor lighting (export_stages / 0x80082E60, 0x8008364C), when the stage has it.
    pub floor_light: Option<FloorLight>,
    /// An arena with walls (the extra levels): both fighters (metres) and a margin; stage surfaces above
    /// the floor in a tube from the camera to either fighter, nearer the camera than that fighter by more than the
    /// margin, are cut away, so walls and props between the camera and the fight do not hide it.
    pub cutaway: Option<([Vec3; 2], f32)>,
    /// With the cutaway: cut everything above the floor nearer the camera than the nearer fighter (less the margin),
    /// not just the tubes (the camera was raised over the walls).
    pub cutaway_all: bool,
    /// Stage surfaces above the floor beyond this z (metres; the camera's side of a route) are not drawn.
    pub near_cut: Option<f32>,
    /// The stage's light for the fighters (None: a plain key light).
    pub light: Option<StageLight>,
    /// The fighters' shadows on the floor (None: none).
    pub shadow: Option<Shadow>,
}

/// Tekken Tag's floor cells (metres): a cell shows the level of detail its square's mean depth picks (0x340408: the
/// square `half` each way from its centre; past `limits[k]` level k + 2 when the floor has it); with a `window` of
/// cells of size `cell` (0: none), each cell is moved by whole windows into the window on `centre` (0x341218), and
/// not drawn further than `reach` from it.
#[derive(Clone, Copy, Debug)]
pub struct FloorLevels {
    pub limits: [f32; 4],
    pub half: [f32; 2],
    pub cell: [f32; 2],
    pub window: [f32; 2],
    pub reach: f32,
    pub centre: [f32; 2],
}

/// The extra stages' floor lighting: mode 0 fades with the view depth (128 + T[(sz - 10000) * 6710 >> 16] * shade /
/// 128), mode 1 lights from each fighter (2^27 / d^2 each, at most 254), mode 2 from their midpoint (0x7A120000 / d^2,
/// at most 192); 128 = as drawn. Distances in game units (1 mm).
#[derive(Clone, Copy, Debug)]
pub struct FloorLight {
    pub mode: u32,
    pub shade: f32,
    /// The fighters' places on the floor (metres, x and z).
    pub fighters: [[f32; 2]; 2],
    /// The fade table 0x80056564, every 32nd of its 1024 bytes.
    pub table: [f32; 32],
}

impl Camera {
    /// A camera with the backdrop at the origin.
    pub fn new(eye: Vec3, target: Vec3, fov_y: f32) -> Self {
        Self {
            eye,
            target,
            fov_y,
            y_stretch: 1.0,
            roll: 0.0,
            sky: Mat4::IDENTITY,
            sky_y_scale: 1.0,
            sky_first: false,
            sky_cull: true,
            sky_depth: false,
            sky_floor_depth: false,
            background: None,
            floor: Mat4::IDENTITY,
            hit_light: None,
            floor_clip: None,
            floor_levels: None,
            fog: None,
            gamma: false,
            grass: None,
            floor_light: None,
            cutaway: None,
            cutaway_all: false,
            near_cut: None,
            light: None,
            shadow: None,
        }
    }

    /// The projection for a picture of `aspect` (width / height).
    pub fn projection(&self, aspect: f32) -> Mat4 {
        Mat4::from_scale(Vec3::new(1.0, self.y_stretch, 1.0)) * glam::camera::rh::proj::directx::perspective(self.fov_y, aspect, 0.05, 400.0)
    }

    /// The view matrix: a right-handed look-at, y up, then the roll.
    pub fn view(&self) -> Mat4 {
        Mat4::from_rotation_z(self.roll) * glam::camera::rh::view::look_at_mat4(self.eye, self.target, Vec3::Y)
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Globals {
    view_proj: [[f32; 4]; 4],     // proj · view
    sky_view_proj: [[f32; 4]; 4], // proj · drum Y scale · view · drum placement
    floor: [[f32; 4]; 4],         // stage mesh placement
    light_dir: [f32; 4],
    hit_colour: [f32; 4],
    hit_pos: [f32; 4],
    floor_clip: [f32; 4],
    floor_light: [f32; 4], // mode (0 none, 1 depth fade, 2 fighters, 3 midpoint), shade, -, -
    fighters: [f32; 4],    // x, z of both fighters (metres)
    eye: [f32; 4],
    forward: [f32; 4],
    fade: [[f32; 4]; 8],
    cutaway: [f32; 4],   // x, y, z (metres) of fighter 1, margin (0 = off)
    cutaway2: [f32; 4],  // x, y, z of fighter 2, 1 = cut all nearer than the nearer fighter
    near_cut: [f32; 4],  // z beyond which stage surfaces above the floor go, 1 = on
    key_color: [f32; 4], // the stage light's key colour, w = 1 when there is one
    ambient: [f32; 4],
    material: [f32; 4],
    shadow: [f32; 4], // the light's x / y and z / y, the floor's height, 1 = on
    shadow_colour: [f32; 4],
    // Tekken Tag's shadow picture: its centre's x, z (metres), 1 / its half size, the darkening (0: none)
    shadow_map: [f32; 4],
    // Tekken Tag's floor cells: the depth limits; the half size of a cell's square (x, z), 1 = on, the reach; the
    // window (cells x, z) and the cell's size; the window's centre (x, z), the picture's aspect
    floor_levels: [f32; 4],
    floor_cell: [f32; 4],
    floor_window: [f32; 4],
    floor_centre: [f32; 4],
    // Tekken Tag's two other lights: towards each, its colour
    lights: [[f32; 4]; 4],
    // Tekken Tag's arena: the eye in the drum's placement (metres), the camera's x and y axes (world)
    sky_eye: [f32; 4],
    view_x: [f32; 4],
    view_y: [f32; 4],
    // Tekken Tag's arena fog: its colour (linear), the coefficient's line (w = 1 when on)
    fog_colour: [f32; 4],
    fog_line: [f32; 4],
    // x 1: the 3D pass on the frame's sRGB bytes (Camera::gamma)
    gamma: [f32; 4],
    // Eddy's grass: the first corner (x, z), a cell (x, z); corners a row, rows, the lattice's height (m), 1 when on;
    // each corner's move (m)
    grass_grid: [f32; 4],
    grass_info: [f32; 4],
    grass: [[f32; 4]; GRASS],
    // OpenKumite's enhancements: x 1 soft shadows
    enhance: [f32; 4],
}

/// Eddy's grass lattice's corners at most (17 x 17).
pub const GRASS: usize = 289;

/// Eddy's grass (0x3D43A8, 0x3E13C8): the floor cells drawn through a lattice of x, z cells and y 0..`size` (m), a
/// top corner at each corner of the cells from `first` (x, z) by `cell`, `row` a row in `rows`, its move `moves` (m); a vertex
/// moves by its cell's top corners' moves (bilinearly) x its height / `size`.
#[derive(Clone, Copy, Debug)]
pub struct Grass {
    pub first: [f32; 2],
    pub cell: [f32; 2],
    pub row: u32,
    pub rows: u32,
    pub size: f32,
    pub moves: [[f32; 4]; GRASS],
}

/// The 3D pass's pipelines for one colour target format.
struct Pipes {
    skinned_pipeline: wgpu::RenderPipeline,
    skinned_blend_pipeline: wgpu::RenderPipeline,
    /// The fighters' shadows.
    shadow_pipeline: wgpu::RenderPipeline,
    floor_shadow_pipeline: wgpu::RenderPipeline,
    /// Force's blob shadows: the floor darkened (the original hardware's subtraction).
    blob_pipeline: wgpu::RenderPipeline,
    static_pipeline: wgpu::RenderPipeline,
    sky_pipeline: wgpu::RenderPipeline,
    /// The drum with depth (`Camera::sky_depth`), and the stage meshes' depth alone (no colour written).
    sky_depth_pipeline: wgpu::RenderPipeline,
    static_depth_pipeline: wgpu::RenderPipeline,
    /// The stage's added and multiplying strips: on the floor (vs_static) and on the model (vs_sky), each kind.
    stage_blends: Vec<wgpu::RenderPipeline>,
    backdrop_pipeline: wgpu::RenderPipeline,
    negative_pipeline: wgpu::RenderPipeline,
    line_pipeline: wgpu::RenderPipeline,
    particle_alpha: wgpu::RenderPipeline,
    particle_add: wgpu::RenderPipeline,
    soft_alpha: wgpu::RenderPipeline,
    soft_add: wgpu::RenderPipeline,
    /// Anti-aliased: the resolved picture drawn back into the samples (after the HUD under the fighters).
    reload: wgpu::RenderPipeline,
}

/// RGBA texture with its bind group.
pub struct Texture {
    bind_group: wgpu::BindGroup,
    /// The same texture through the smooth (linear) sampler, for [`RenderSettings::smooth_textures`].
    bind_group_smooth: wgpu::BindGroup,
    /// Through its mipmaps, trilinear and anisotropic, for [`RenderSettings::mipmaps`].
    bind_group_filtered: wgpu::BindGroup,
    texture: wgpu::Texture,
    /// With mipmaps: its texels (the face animation's copies make the levels again).
    texels: Option<std::sync::Mutex<Vec<u8>>>,
}

impl Texture {
    /// Its bind group for the texture filter (0 sharp, 1 smooth, 2 filtered).
    fn group(&self, filter: usize) -> &wgpu::BindGroup {
        match filter {
            0 => &self.bind_group,
            1 => &self.bind_group_smooth,
            _ => &self.bind_group_filtered,
        }
    }
}

/// How frames are drawn (the OPTION menu's GRAPHICS, or the command line).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RenderSettings {
    /// The 3D scene's resolution over the window's (0.25 .. 2: below 1 coarser, above 1 supersampled).
    pub scale: f32,
    /// Wait for the display's refresh.
    pub vsync: bool,
    /// Textures filtered (linear) instead of the original hardware's sharp texels.
    pub smooth_textures: bool,
    /// Anti-aliasing: the 3D scene's samples a pixel (1: none; 2 or 4, as the GPU has them).
    pub msaa: u32,
    /// Textures made from now on have mipmaps (3 levels), drawn trilinear and 16x anisotropic.
    pub mipmaps: bool,
    /// Tekken Tag's shadows soft: their picture finer, its edge averaged over 3 cm.
    pub soft_shadows: bool,
    /// The added lights (sparks, flames, the stages' lamps) glow: blurred and added again.
    pub glow: bool,
}

impl Default for RenderSettings {
    fn default() -> Self {
        Self { scale: 1.0, vsync: true, smooth_textures: false, msaa: 1, mipmaps: false, soft_shadows: false, glow: false }
    }
}

/// The 3D scene's own target when the render scale is not 1 or it is anti-aliased: colour (with a non-sRGB view for
/// the HUD drawn under the fighters), depth, and the bind group the final pass samples it through.
/// The glow's targets at half the scene's size: its picture and the blur's (and their bind groups), the depth.
struct Glow {
    size: [u32; 2],
    views: [wgpu::TextureView; 2],
    groups: [wgpu::BindGroup; 2],
    depth: wgpu::TextureView,
}

struct SceneTarget {
    size: [u32; 2],
    colour: wgpu::Texture,
    depth: wgpu::TextureView,
    group: wgpu::BindGroup,
    /// Anti-aliased: the samples' colour and depth (their count, the pass's format), resolved into `colour`; and
    /// `colour` as the reload reads it, through either view.
    msaa: Option<(u32, wgpu::TextureFormat, wgpu::TextureView, wgpu::TextureView)>,
    reload: [wgpu::BindGroup; 2],
}

pub struct SkinnedModel {
    vbuf: wgpu::Buffer,
    ibuf: wgpu::Buffer,
    count: u32,
    /// The indices from here to `count` are drawn blended, after every model's others.
    blend_from: u32,
    bones: wgpu::Buffer,
    bones_group: wgpu::BindGroup,
    texture: Arc<Texture>,
    /// The shadow mesh (vertices, indices, count), on the same bones.
    shadow: Option<(wgpu::Buffer, wgpu::Buffer, u32)>,
}

impl SkinnedModel {
    /// The model's texture (its own: the face animation writes into it).
    pub fn texture(&self) -> &Texture {
        &self.texture
    }
}

pub struct StaticMesh {
    vbuf: wgpu::Buffer,
    ibuf: wgpu::Buffer,
    count: u32,
    texture: Arc<Texture>,
    /// Drawn without depth, sorted far to near each frame as the original hardware's ordering table does (the backdrop
    /// drum): its positions and triangles.
    sorted: Option<(Vec<Vec3>, Vec<[u32; 3]>)>,
    /// 0 drawn over what is behind it; 1 added to it, 2 multiplying it (Tekken Tag's lights and gloss, its painted
    /// shadows: drawn after the stage, depth-tested, never written).
    blend: u8,
}

impl StaticMesh {
    /// The mesh as one added to (1) or multiplying (2) what is behind it.
    pub fn blended(self, blend: u8) -> Self {
        Self { blend, ..self }
    }

    /// Another texture on the mesh (a stage's flame: its next picture).
    pub fn set_texture(&mut self, texture: Arc<Texture>) {
        self.texture = texture;
    }
}

pub const MAX_BONES: usize = 128;

/// The final pass: the scene's target stretched over the window (a full-screen triangle).
const BLIT_WGSL: &str = "
@group(0) @binding(0) var tex: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;
struct Out { @builtin(position) pos: vec4<f32>, @location(0) uv: vec2<f32> };
@vertex fn vs(@builtin(vertex_index) i: u32) -> Out {
    let uv = vec2(f32((i << 1u) & 2u), f32(i & 2u));
    var o: Out;
    o.pos = vec4(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, 0.0, 1.0);
    o.uv = uv;
    return o;
}
@fragment fn fs(i: Out) -> @location(0) vec4<f32> { return textureSample(tex, samp, i.uv); }
// the glow (OpenKumite's): the added light past a quarter (not a floor's faint gloss) blurred along x, then y (9 taps
// 1.5 texels apart, gaussian), then added to the scene
fn blur(uv: vec2<f32>, dir: vec2<f32>, cut: f32) -> vec4<f32> {
    let px = 1.5 * dir / vec2<f32>(textureDimensions(tex));
    let w = array(0.227, 0.194, 0.121, 0.054, 0.016);
    var c = max(textureSample(tex, samp, uv) - cut, vec4(0.0)) * w[0];
    for (var k = 1; k < 5; k++) {
        c += (max(textureSample(tex, samp, uv + px * f32(k)) - cut, vec4(0.0)) + max(textureSample(tex, samp, uv - px * f32(k)) - cut, vec4(0.0))) * w[k];
    }
    return c;
}
@fragment fn fs_blur_x(i: Out) -> @location(0) vec4<f32> { return blur(i.uv, vec2(1.0, 0.0), 0.25); }
@fragment fn fs_blur_y(i: Out) -> @location(0) vec4<f32> { return blur(i.uv, vec2(0.0, 1.0), 0.0); }
@fragment fn fs_glow(i: Out) -> @location(0) vec4<f32> { return vec4(textureSample(tex, samp, i.uv).rgb * 0.8, 0.0); }
";
/// The glow's picture (its half-resolution targets).
const GLOW: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const DEPTH: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
/// Tekken Tag's shadow picture: its texels a side, and the half size of the floor it covers (metres).
const SHADOW_MAP: u32 = 1024;
/// ... with soft shadows ([`RenderSettings::soft_shadows`]).
const SHADOW_MAP_SOFT: u32 = 2048;
const SHADOW_HALF: f32 = 6.0;
/// Mipmapped textures' levels: the texels and three halvings (more would blend an atlas's parts together).
const MIPS: u32 = 4;

pub struct Renderer {
    /// What the window surfaces are made with (Android makes its window again after the background).
    instance: wgpu::Instance,
    /// Window surface, or `None` when rendering offscreen (tests, screenshots).
    surface: Option<wgpu::Surface<'static>>,
    /// Offscreen colour target (headless mode).
    offscreen: Option<wgpu::Texture>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    /// What the scene is drawn as: the surface's format, or its sRGB view when it has no sRGB format.
    frame_format: wgpu::TextureFormat,
    depth: wgpu::TextureView,
    globals: wgpu::Buffer,
    globals_group: wgpu::BindGroup,
    texture_layout: wgpu::BindGroupLayout,
    bones_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    /// Linear, mipmapped, tiling both ways: textures that ask for it (the extra levels').
    sampler_filtered: wgpu::Sampler,
    /// Linear, the atlas's addressing: the smooth textures setting.
    sampler_smooth: wgpu::Sampler,
    /// Sharp texels tiling both ways, and the same filtered.
    sampler_tiled: [wgpu::Sampler; 2],
    /// Trilinear, 16x anisotropic: the atlas's addressing, and tiling both ways.
    sampler_aniso: [wgpu::Sampler; 2],
    /// The scene's upscale (nearest: a lower resolution shows its pixels) and downscale (linear) to the window.
    blit_nearest: wgpu::Sampler,
    blit_linear: wgpu::Sampler,
    blit_pipeline: wgpu::RenderPipeline,
    /// The glow ([`RenderSettings::glow`]): its blurs (x, y), its adding into the scene (the frame's format, its
    /// plain one), the 3D pass's pipelines for its picture (made when first wanted), its targets.
    glow_blur: [wgpu::RenderPipeline; 2],
    glow_add: [wgpu::RenderPipeline; 2],
    glow_pipes: Option<Pipes>,
    glow: Option<Glow>,
    settings: RenderSettings,
    scene: Option<SceneTarget>,
    /// The 3D pass's pipelines: drawing in the frame's linear colour, and (Tekken Tag's) in its sRGB bytes, so that
    /// blending and modulation happen on them as on the PS2's GS ([`Camera::gamma`]).
    pipes: [Pipes; 2],
    /// Makes [`Renderer::pipes`] for a format and a sample count.
    make_pipes: Box<dyn Fn(wgpu::TextureFormat, u32) -> Pipes>,
    /// The sample counts the scene can be drawn with (1, and 2 or 4 where the GPU has them).
    msaa_ok: Vec<u32>,
    /// Tekken Tag's shadows (0x340E28): each fighter's silhouette along the light into a channel of a picture of the
    /// floor's plane around the camera's target, then the floor drawn again darkened where the picture covers it.
    shadow_map: wgpu::Texture,
    shadow_map_group: wgpu::BindGroup,
    shadow_map_pipelines: Vec<wgpu::RenderPipeline>,
    /// This frame's blob shadows ([`Renderer::set_blob_shadows`]).
    blobs: Vec<StaticVertex>,
    /// 2D pictures behind everything in the next frames, drawn first in this order (an arcade stage's sky): triangles
    /// in the original's 4:3 picture's own space (x and y -1..1 across it, y up; a wider window shows more of x),
    /// each with its texture.
    pub backdrop: Vec<(Arc<Texture>, Vec<StaticVertex>)>,
    /// A colour the picture is taken from, over the scene (Tekken Tag's ending op 24: ALPHA (Cs - Cd) x Ad).
    pub negative: Option<[f32; 3]>,
    /// A white texel for the untextured particles.
    white: Option<Arc<Texture>>,
    adapter_name: String,
    /// The GPU's kind (discrete, integrated, software; a browser does not tell).
    device_type: wgpu::DeviceType,
    /// Copy the next presented window frame back to memory (see [`Renderer::request_capture`]).
    capture_next: bool,
    captured: Option<Result<Image>>,
    pub clear: wgpu::Color,
}

impl Renderer {
    /// Create the renderer for a window (anything wgpu can make a surface from).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn new(target: impl Into<wgpu::SurfaceTarget<'static>>, width: u32, height: u32) -> Result<Self> {
        pollster::block_on(Self::new_async(target, width, height))
    }

    /// The same, waiting for the GPU without blocking (a browser cannot block).
    pub async fn new_async(target: impl Into<wgpu::SurfaceTarget<'static>>, width: u32, height: u32) -> Result<Self> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
        let surface = instance.create_surface(target)?;
        Self::init(instance, Some(surface), width, height).await
    }

    /// Create an offscreen renderer of a fixed size; read frames back with [`Renderer::capture`].
    #[cfg(not(target_arch = "wasm32"))]
    pub fn headless(width: u32, height: u32) -> Result<Self> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
        pollster::block_on(Self::init(instance, None, width, height))
    }

    async fn init(instance: wgpu::Instance, surface: Option<wgpu::Surface<'static>>, width: u32, height: u32) -> Result<Self> {
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: surface.as_ref(),
                force_fallback_adapter: false,
                ..Default::default()
            })
            .await
            .context("no GPU adapter")?;
        let info = adapter.get_info();
        log::info!("GPU: {info:?}");
        let adapter_name = format!("{} ({:?})", info.name, info.backend);
        // (in a browser, WebGL2's limits so it runs on either WebGPU or WebGL2)
        let base = if cfg!(target_arch = "wasm32") { wgpu::Limits::downlevel_webgl2_defaults() } else { wgpu::Limits::downlevel_defaults() };
        // (2x anti-aliasing, where the adapter has it, needs its own formats' features)
        let required_features = adapter.features() & wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES;
        let (device, queue) =
            adapter.request_device(&wgpu::DeviceDescriptor { label: Some("openkumite"), required_features, required_limits: base.using_resolution(adapter.limits()), ..Default::default() }).await?;
        let max = device.limits().max_texture_dimension_2d;
        let (width, height) = (width.clamp(1, max), height.clamp(1, max));
        let config = match &surface {
            Some(surface) => {
                let mut c = surface.get_default_config(&adapter, width, height).context("surface not supported by the adapter")?;
                c.present_mode = wgpu::PresentMode::AutoVsync;
                // allow copying window frames back (screenshots), when the surface supports it
                if surface.get_capabilities(&adapter).usages.contains(wgpu::TextureUsages::COPY_SRC) {
                    c.usage |= wgpu::TextureUsages::COPY_SRC;
                }
                // overlays (egui) blend in gamma space: they draw through a non-sRGB view of the frame, where the
                // device allows other view formats (WebGL 2 does not: there they draw into the frame as it is)
                let views = adapter.get_downlevel_capabilities().flags.contains(wgpu::DownlevelFlags::SURFACE_VIEW_FORMATS);
                if views && c.format.remove_srgb_suffix() != c.format {
                    c.view_formats.push(c.format.remove_srgb_suffix());
                }
                // a surface without an sRGB format (a WebGPU canvas: bgra8unorm): the frame is drawn through an
                // sRGB view of it, so its linear colours are encoded as everywhere else
                if views && !c.format.is_srgb() && c.format.add_srgb_suffix() != c.format {
                    c.view_formats.push(c.format.add_srgb_suffix());
                }
                surface.configure(&device, &c);
                c
            }
            None => wgpu::SurfaceConfiguration {
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                color_space: Default::default(),
                width,
                height,
                present_mode: wgpu::PresentMode::AutoVsync,
                desired_maximum_frame_latency: 2,
                alpha_mode: wgpu::CompositeAlphaMode::Auto,
                view_formats: vec![wgpu::TextureFormat::Rgba8Unorm],
            },
        };
        let offscreen = surface.is_none().then(|| make_offscreen(&device, &config));
        let srgb = config.format.add_srgb_suffix();
        let frame_format = if config.view_formats.contains(&srgb) { srgb } else { config.format };
        let depth = make_depth(&device, &config);

        let globals = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("globals"),
            size: std::mem::size_of::<Globals>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let globals_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("globals"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                count: None,
            }],
        });
        let globals_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("globals"),
            layout: &globals_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: globals.as_entire_binding() }],
        });
        let texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("texture"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry { binding: 1, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None },
            ],
        });
        let bones_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("bones"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                count: None,
            }],
        });
        // original-hardware look: nearest filtering, clamped atlas.
        let sampler =
            device.create_sampler(&wgpu::SamplerDescriptor { label: Some("nearest"), address_mode_u: wgpu::AddressMode::Repeat, address_mode_v: wgpu::AddressMode::ClampToEdge, ..Default::default() });

        let sampler_filtered = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("filtered"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            anisotropy_clamp: 8,
            ..Default::default()
        });
        let sampler_smooth = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("smooth"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let tiled = |filter| {
            device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("tiled"),
                address_mode_u: wgpu::AddressMode::Repeat,
                address_mode_v: wgpu::AddressMode::Repeat,
                mag_filter: filter,
                min_filter: filter,
                ..Default::default()
            })
        };
        let sampler_tiled = [tiled(wgpu::FilterMode::Nearest), tiled(wgpu::FilterMode::Linear)];
        let sampler_aniso = [wgpu::AddressMode::ClampToEdge, wgpu::AddressMode::Repeat].map(|v| {
            device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("aniso"),
                address_mode_u: wgpu::AddressMode::Repeat,
                address_mode_v: v,
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                mipmap_filter: wgpu::MipmapFilterMode::Linear,
                anisotropy_clamp: 16,
                ..Default::default()
            })
        });
        let blit_nearest = device.create_sampler(&wgpu::SamplerDescriptor { label: Some("blit nearest"), ..Default::default() });
        let blit_linear =
            device.create_sampler(&wgpu::SamplerDescriptor { label: Some("blit linear"), mag_filter: wgpu::FilterMode::Linear, min_filter: wgpu::FilterMode::Linear, ..Default::default() });
        let blit_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("blit"), source: wgpu::ShaderSource::Wgsl(BLIT_WGSL.into()) });
        let blit_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("blit"), bind_group_layouts: &[Some(&texture_layout)], immediate_size: 0 });
        let blit_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("blit"),
            layout: Some(&blit_layout),
            vertex: wgpu::VertexState { module: &blit_shader, entry_point: Some("vs"), compilation_options: Default::default(), buffers: &[] },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &blit_shader,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState { format: frame_format, blend: None, write_mask: wgpu::ColorWrites::ALL })],
            }),
            multiview_mask: None,
            cache: None,
        });
        // (the glow's passes: the blit's triangle)
        let post = |fs: &str, format: wgpu::TextureFormat, blend: Option<wgpu::BlendState>| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(fs),
                layout: Some(&blit_layout),
                vertex: wgpu::VertexState { module: &blit_shader, entry_point: Some("vs"), compilation_options: Default::default(), buffers: &[] },
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &blit_shader,
                    entry_point: Some(fs),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState { format, blend, write_mask: wgpu::ColorWrites::ALL })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let glow_blur = ["fs_blur_x", "fs_blur_y"].map(|fs| post(fs, GLOW, None));
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("openkumite"), source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()) });
        let layout = |groups: &[Option<&wgpu::BindGroupLayout>]| device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: groups, immediate_size: 0 });
        let skinned_layout = layout(&[Some(&globals_layout), Some(&texture_layout), Some(&bones_layout)]);
        let static_layout = layout(&[Some(&globals_layout), Some(&texture_layout)]);

        let skin_attrs = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2, 2 => Uint16x4, 3 => Float32x4, 4 => Float32x3];
        let static_attrs = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2, 2 => Float32x4, 3 => Float32x4, 4 => Float32x4];
        use wgpu::CompareFunction::{Always, Less, LessEqual};
        let skin_vb = [wgpu::VertexBufferLayout { array_stride: std::mem::size_of::<SkinVertex>() as u64, step_mode: wgpu::VertexStepMode::Vertex, attributes: &skin_attrs }];
        let line_attrs = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x4];
        let line_layout = layout(&[Some(&globals_layout)]);
        let tri = wgpu::PrimitiveTopology::TriangleList;
        // Tekken Tag's: a fighter a channel (its triangles cover it once, however they overlap), no depth
        let (shadow_map, shadow_map_group) = shadow_picture(&device, &texture_layout, &sampler_smooth, SHADOW_MAP);
        let shadow_map_pipelines = [wgpu::ColorWrites::RED, wgpu::ColorWrites::GREEN, wgpu::ColorWrites::BLUE, wgpu::ColorWrites::ALPHA]
            .map(|write_mask| {
                device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("shadow picture"),
                    layout: Some(&skinned_layout),
                    vertex: wgpu::VertexState { module: &shader, entry_point: Some("vs_shadow_map"), compilation_options: Default::default(), buffers: &[Some(skin_vb[0].clone())] },
                    primitive: wgpu::PrimitiveState { topology: tri, cull_mode: None, ..Default::default() },
                    depth_stencil: None,
                    multisample: Default::default(),
                    fragment: Some(wgpu::FragmentState {
                        module: &shader,
                        entry_point: Some("fs_shadow_map"),
                        compilation_options: Default::default(),
                        targets: &[Some(wgpu::ColorTargetState { format: wgpu::TextureFormat::Rgba8Unorm, blend: None, write_mask })],
                    }),
                    multiview_mask: None,
                    cache: None,
                })
            })
            .to_vec();
        // the 3D pass's pipelines for a colour target format and a sample count (kept: anti-aliasing changes it)
        let (dev, reload_layout, blit_shader) = (device.clone(), blit_layout.clone(), blit_shader.clone());
        let make_pipes = move |format: wgpu::TextureFormat, samples: u32| -> Pipes {
            let device = &dev;
            let skin_vb = [wgpu::VertexBufferLayout { array_stride: std::mem::size_of::<SkinVertex>() as u64, step_mode: wgpu::VertexStepMode::Vertex, attributes: &skin_attrs }];
            let static_vb = [wgpu::VertexBufferLayout { array_stride: std::mem::size_of::<StaticVertex>() as u64, step_mode: wgpu::VertexStepMode::Vertex, attributes: &static_attrs }];
            let line_vb = [wgpu::VertexBufferLayout { array_stride: std::mem::size_of::<LineVertex>() as u64, step_mode: wgpu::VertexStepMode::Vertex, attributes: &line_attrs }];
            let make = |label: &str,
                        layout: &wgpu::PipelineLayout,
                        vs: &str,
                        fs: &str,
                        buffers: &[wgpu::VertexBufferLayout],
                        depth_write: bool,
                        depth_compare: wgpu::CompareFunction,
                        topology: wgpu::PrimitiveTopology,
                        blend: Option<wgpu::BlendState>| {
                // anti-aliased, the cut-outs' edges as coverage (the alpha itself not written); not the backdrops'
                // (fs_unlit: their half-transparent texels' edges would show as lines)
                let coverage = samples > 1 && matches!(fs, "fs_lit" | "fs_static");
                device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some(label),
                    layout: Some(layout),
                    vertex: wgpu::VertexState { module: &shader, entry_point: Some(vs), compilation_options: Default::default(), buffers: &buffers.iter().cloned().map(Some).collect::<Vec<_>>() },
                    primitive: wgpu::PrimitiveState { topology, cull_mode: None, ..Default::default() },
                    depth_stencil: Some(wgpu::DepthStencilState {
                        format: DEPTH,
                        depth_write_enabled: Some(depth_write),
                        depth_compare: Some(depth_compare),
                        stencil: Default::default(),
                        bias: Default::default(),
                    }),
                    multisample: wgpu::MultisampleState { count: samples, alpha_to_coverage_enabled: coverage, ..Default::default() },
                    fragment: Some(wgpu::FragmentState {
                        module: &shader,
                        entry_point: Some(fs),
                        compilation_options: wgpu::PipelineCompilationOptions { constants: if coverage { &[("coverage", 1.0)] } else { &[] }, ..Default::default() },
                        targets: &[Some(wgpu::ColorTargetState { format, blend, write_mask: if coverage { wgpu::ColorWrites::COLOR } else { wgpu::ColorWrites::ALL } })],
                    }),
                    multiview_mask: None,
                    cache: None,
                })
            };
            let skinned_pipeline = make("skinned", &skinned_layout, "vs_skinned", "fs_lit", &skin_vb, true, Less, tri, None);
            // a character's blended primitive (Tekken Tag's): over what is drawn by its texture's alpha, depth not written; its
            // overlays lie on the opaque layer, which the GS's ZTST GEQUAL lets them cover
            let skinned_blend_pipeline = make("skinned blend", &skinned_layout, "vs_skinned", "fs_lit_blend", &skin_vb, false, LessEqual, tri, Some(wgpu::BlendState::ALPHA_BLENDING));
            let static_pipeline = make("static", &static_layout, "vs_static", "fs_static", &static_vb, true, Less, tri, None);
            let sky_pipeline = make("sky", &static_layout, "vs_sky", "fs_unlit", &static_vb, false, Always, tri, None);
            let sky_depth_pipeline = make("sky depth", &static_layout, "vs_sky", "fs_unlit", &static_vb, true, Less, tri, None);
            // (the colour kept as it is: only the depth is written)
            let keep = wgpu::BlendComponent { src_factor: wgpu::BlendFactor::Zero, dst_factor: wgpu::BlendFactor::One, operation: wgpu::BlendOperation::Add };
            let static_depth_pipeline = make("static depth", &static_layout, "vs_static", "fs_static", &static_vb, true, Less, tri, Some(wgpu::BlendState { color: keep, alpha: keep }));
            let backdrop_pipeline = make("backdrop", &static_layout, "vs_backdrop", "fs_unlit", &static_vb, false, Always, tri, None);
            let sub = wgpu::BlendComponent { src_factor: wgpu::BlendFactor::One, dst_factor: wgpu::BlendFactor::One, operation: wgpu::BlendOperation::Subtract };
            let negative_pipeline = make("negative", &static_layout, "vs_backdrop", "fs_unlit", &static_vb, false, Always, tri, Some(wgpu::BlendState { color: sub, alpha: keep }));
            let line_pipeline = make("lines", &line_layout, "vs_line", "fs_line", &line_vb, false, Always, wgpu::PrimitiveTopology::LineList, None);
            // particles: depth-tested against the scene, never written
            let particle_alpha = make("particles alpha", &static_layout, "vs_static", "fs_particle_alpha", &static_vb, false, Less, tri, Some(wgpu::BlendState::ALPHA_BLENDING));
            let add = wgpu::BlendComponent { src_factor: wgpu::BlendFactor::One, dst_factor: wgpu::BlendFactor::One, operation: wgpu::BlendOperation::Add };
            let particle_add = make("particles add", &static_layout, "vs_static", "fs_particle_add", &static_vb, false, Less, tri, Some(wgpu::BlendState { color: add, alpha: keep }));
            let soft_alpha = make("particles soft alpha", &static_layout, "vs_static", "fs_particle_soft_alpha", &static_vb, false, Less, tri, Some(wgpu::BlendState::ALPHA_BLENDING));
            let soft_add = make("particles soft add", &static_layout, "vs_static", "fs_particle_soft_add", &static_vb, false, Less, tri, Some(wgpu::BlendState { color: add, alpha: keep }));
            // shadows: opaque, depth-tested against the stage (the original sorts them behind what stands nearer), never
            // written
            let shade = wgpu::BlendComponent { src_factor: wgpu::BlendFactor::Zero, dst_factor: wgpu::BlendFactor::SrcAlpha, operation: wgpu::BlendOperation::Add };
            // the strips lie on the faces they light or shade (Hwoarang's floor: its stone added over the same quads), so
            // they pass where the depth is equal
            let stage_blends = ["vs_static", "vs_sky"]
                .into_iter()
                .flat_map(|vs| {
                    [("fs_stage_add", add), ("fs_stage_shade", shade)]
                        .map(|(fs, c)| make("stage strips", &static_layout, vs, fs, &static_vb, false, LessEqual, tri, Some(wgpu::BlendState { color: c, alpha: keep })))
                })
                .collect();
            let shadow_pipeline = make("shadows", &skinned_layout, "vs_shadow", "fs_shadow", &skin_vb, false, Less, tri, None);
            // the frame times a fragment's factor (the floor's shadow, the blobs)
            let times = wgpu::BlendComponent { src_factor: wgpu::BlendFactor::Zero, dst_factor: wgpu::BlendFactor::Src, operation: wgpu::BlendOperation::Add };
            // the floor again over itself (a hair nearer)
            let floor_shadow_pipeline = make("floor shadow", &static_layout, "vs_floor_shadow", "fs_floor_shadow", &static_vb, false, Less, tri, Some(wgpu::BlendState { color: times, alpha: keep }));
            // blobs: the frame times the fragment's factor
            let blob_pipeline = make("blob shadows", &static_layout, "vs_world", "fs_blob", &static_vb, false, Less, tri, Some(wgpu::BlendState { color: times, alpha: keep }));
            // anti-aliased: the picture so far (with the HUD drawn under the fighters) back into the samples
            let reload = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("reload"),
                layout: Some(&reload_layout),
                vertex: wgpu::VertexState { module: &blit_shader, entry_point: Some("vs"), compilation_options: Default::default(), buffers: &[] },
                primitive: Default::default(),
                depth_stencil: Some(wgpu::DepthStencilState { format: DEPTH, depth_write_enabled: Some(false), depth_compare: Some(Always), stencil: Default::default(), bias: Default::default() }),
                multisample: wgpu::MultisampleState { count: samples, ..Default::default() },
                fragment: Some(wgpu::FragmentState {
                    module: &blit_shader,
                    entry_point: Some("fs"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::ALL })],
                }),
                multiview_mask: None,
                cache: None,
            });
            Pipes {
                skinned_pipeline,
                skinned_blend_pipeline,
                shadow_pipeline,
                floor_shadow_pipeline,
                blob_pipeline,
                static_pipeline,
                sky_pipeline,
                sky_depth_pipeline,
                static_depth_pipeline,
                stage_blends,
                backdrop_pipeline,
                negative_pipeline,
                line_pipeline,
                particle_alpha,
                particle_add,
                soft_alpha,
                soft_add,
                reload,
            }
        };
        // (the frame's linear colour; its sRGB bytes where it has a plain view, else the same)
        let plain = config.format.remove_srgb_suffix();
        let gamma = if plain == config.format || config.view_formats.contains(&plain) { plain } else { frame_format };
        let pipes = [make_pipes(frame_format, 1), make_pipes(gamma, 1)];
        let add = wgpu::BlendComponent { src_factor: wgpu::BlendFactor::One, dst_factor: wgpu::BlendFactor::One, operation: wgpu::BlendOperation::Add };
        let keep = wgpu::BlendComponent { src_factor: wgpu::BlendFactor::Zero, dst_factor: wgpu::BlendFactor::One, operation: wgpu::BlendOperation::Add };
        let glow_add = [frame_format, gamma].map(|f| post("fs_glow", f, Some(wgpu::BlendState { color: add, alpha: keep })));
        // the anti-aliasing the scene's formats can have (and resolve)
        let samples_ok = |n: u32| {
            let ok = |f: wgpu::TextureFormat, resolve: bool| {
                let fl = adapter.get_texture_format_features(f).flags;
                fl.sample_count_supported(n) && (!resolve || fl.contains(wgpu::TextureFormatFeatureFlags::MULTISAMPLE_RESOLVE))
            };
            (n == 4 || device.features().contains(wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES)) && ok(frame_format, true) && ok(gamma, true) && ok(DEPTH, false)
        };
        let msaa_ok = [1, 2, 4].into_iter().filter(|&n| n == 1 || samples_ok(n)).collect();
        Ok(Self {
            instance,
            surface,
            offscreen,
            device,
            queue,
            config,
            frame_format,
            depth,
            globals,
            globals_group,
            texture_layout,
            bones_layout,
            sampler,
            sampler_filtered,
            sampler_smooth,
            sampler_tiled,
            sampler_aniso,
            blit_nearest,
            blit_linear,
            blit_pipeline,
            glow_blur,
            glow_add,
            glow_pipes: None,
            glow: None,
            settings: RenderSettings::default(),
            scene: None,
            pipes,
            make_pipes: Box::new(make_pipes),
            msaa_ok,
            shadow_map,
            shadow_map_group,
            shadow_map_pipelines,
            blobs: Vec::new(),
            backdrop: Vec::new(),
            negative: None,
            white: None,
            adapter_name,
            device_type: info.device_type,
            capture_next: false,
            captured: None,
            clear: wgpu::Color { r: 0.06, g: 0.067, b: 0.085, a: 1.0 },
        }
        .with_white())
    }

    fn with_white(mut self) -> Self {
        self.white = Some(self.texture(1, 1, &[255; 4]));
        self
    }

    fn bind(&self, view: &wgpu::TextureView, sampler: &wgpu::Sampler) -> wgpu::BindGroup {
        self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.texture_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(view) }, wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(sampler) }],
        })
    }

    /// Change how frames are drawn: the render scale, vsync (the surface reconfigured), the texture filter.
    pub fn set_settings(&mut self, s: RenderSettings) {
        let msaa = self.msaa_ok.iter().copied().filter(|&n| n <= s.msaa).max().unwrap_or(1);
        let s = RenderSettings { scale: s.scale.clamp(0.1, 4.0), msaa, ..s };
        if s.msaa != self.settings.msaa {
            self.pipes = [(self.make_pipes)(self.frame_format, msaa), (self.make_pipes)(self.overlay_format(), msaa)];
        }
        if s.soft_shadows != self.settings.soft_shadows {
            let side = if s.soft_shadows { SHADOW_MAP_SOFT } else { SHADOW_MAP };
            (self.shadow_map, self.shadow_map_group) = shadow_picture(&self.device, &self.texture_layout, &self.sampler_smooth, side);
        }
        if s.vsync != self.settings.vsync {
            self.config.present_mode = if s.vsync { wgpu::PresentMode::AutoVsync } else { wgpu::PresentMode::AutoNoVsync };
            if let Some(surface) = &self.surface {
                surface.configure(&self.device, &self.config);
            }
        }
        self.settings = s;
    }

    /// The scene's target for the render scale and the anti-aliasing drawn in `format` (None at 1 without: the frame
    /// itself), made or remade for its size.
    fn scene_target(&mut self, format: wgpu::TextureFormat) -> Option<&SceneTarget> {
        let (scale, samples) = (self.settings.scale, self.settings.msaa);
        if (scale - 1.0).abs() < 1e-3 && samples == 1 {
            self.scene = None;
            return None;
        }
        let max = self.max_texture();
        let size = [((self.config.width as f32 * scale).round() as u32).clamp(1, max), ((self.config.height as f32 * scale).round() as u32).clamp(1, max)];
        if self.scene.as_ref().is_none_or(|t| t.size != size) {
            let overlay = [self.overlay_format()];
            let views: &[wgpu::TextureFormat] = if overlay[0] != self.frame_format { &overlay } else { &[] };
            let colour = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("scene"),
                size: wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: self.frame_format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: views,
            });
            let depth = self
                .device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some("scene depth"),
                    size: wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: DEPTH,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                    view_formats: &[],
                })
                .create_view(&Default::default());
            // a lower resolution shows its pixels (nearest); a higher one is averaged down (linear)
            let sampler = if scale <= 1.0 { &self.blit_nearest } else { &self.blit_linear };
            let group = self.bind(&colour.create_view(&Default::default()), sampler);
            let reload = [colour.create_view(&Default::default()), colour.create_view(&wgpu::TextureViewDescriptor { format: Some(overlay[0]), ..Default::default() })]
                .map(|v| self.bind(&v, &self.blit_nearest));
            self.scene = Some(SceneTarget { size, colour, depth, group, msaa: None, reload });
        }
        let t = self.scene.as_mut()?;
        if samples == 1 {
            t.msaa = None;
        } else if t.msaa.as_ref().is_none_or(|m| m.0 != samples || m.1 != format) {
            let make = |label, format| {
                self.device
                    .create_texture(&wgpu::TextureDescriptor {
                        label: Some(label),
                        size: wgpu::Extent3d { width: t.size[0], height: t.size[1], depth_or_array_layers: 1 },
                        mip_level_count: 1,
                        sample_count: samples,
                        dimension: wgpu::TextureDimension::D2,
                        format,
                        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                        view_formats: &[],
                    })
                    .create_view(&Default::default())
            };
            t.msaa = Some((samples, format, make("scene samples", format), make("scene samples depth", DEPTH)));
        }
        self.scene.as_ref()
    }

    /// The glow's pipelines and targets, made or remade for its size.
    fn glow_target(&mut self, size: [u32; 2]) {
        if self.glow_pipes.is_none() {
            self.glow_pipes = Some((self.make_pipes)(GLOW, 1));
        }
        if self.glow.as_ref().is_some_and(|g| g.size == size) {
            return;
        }
        let make = |format, usage| {
            self.device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some("glow"),
                    size: wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage,
                    view_formats: &[],
                })
                .create_view(&Default::default())
        };
        let views = [(); 2].map(|_| make(GLOW, wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING));
        let groups = [0, 1].map(|k| self.bind(&views[k], &self.blit_linear));
        let depth = make(DEPTH, wgpu::TextureUsages::RENDER_ATTACHMENT);
        self.glow = Some(Glow { size, views, groups, depth });
    }

    /// The GPU's largest texture side: frames and the 3D scene's target are at most this.
    pub fn max_texture(&self) -> u32 {
        self.device.limits().max_texture_dimension_2d
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        // (a window larger than the GPU's textures: the frame at the limit, stretched)
        let (width, height) = (width.min(self.max_texture()), height.min(self.max_texture()));
        self.config.width = width;
        self.config.height = height;
        match &self.surface {
            Some(s) => s.configure(&self.device, &self.config),
            None => self.offscreen = Some(make_offscreen(&self.device, &self.config)),
        }
        self.depth = make_depth(&self.device, &self.config);
    }

    /// The window's surface dropped (None: Android took the window away, the app in the background; draw nothing
    /// meanwhile) or made again for its new window, at its size. The GPU's resources stay.
    pub fn set_surface(&mut self, target: Option<wgpu::SurfaceTarget<'static>>, width: u32, height: u32) -> Result<()> {
        self.surface = None;
        if let Some(t) = target {
            self.surface = Some(self.instance.create_surface(t)?);
            self.resize(width, height);
        }
        Ok(())
    }

    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }

    /// Colour format an [`Overlay`] renders into: the frame's format without sRGB encoding (UI libraries
    /// such as egui blend in gamma space).
    pub fn overlay_format(&self) -> wgpu::TextureFormat {
        let plain = self.config.format.remove_srgb_suffix();
        if plain == self.config.format || self.config.view_formats.contains(&plain) {
            plain
        } else {
            self.config.format
        }
    }

    pub fn size(&self) -> [u32; 2] {
        [self.config.width, self.config.height]
    }

    /// GPU name and backend.
    pub fn adapter_name(&self) -> &str {
        &self.adapter_name
    }

    /// The GPU's kind (discrete, integrated, software; `Other` in a browser).
    pub fn device_type(&self) -> wgpu::DeviceType {
        self.device_type
    }

    /// Copy the next window frame back; collect it with [`Renderer::take_capture`].
    pub fn request_capture(&mut self) {
        self.capture_next = true;
    }

    pub fn take_capture(&mut self) -> Option<Result<Image>> {
        self.captured.take()
    }

    pub fn aspect(&self) -> f32 {
        self.config.width as f32 / self.config.height as f32
    }

    /// Upload an RGBA8 texture (sRGB colours, alpha 0 = transparent texel).
    pub fn texture(&self, width: u32, height: u32, rgba: &[u8]) -> Arc<Texture> {
        self.upload(width, height, rgba, [&self.sampler, &self.sampler_smooth, &self.sampler_aniso[0]])
    }

    /// Upload an RGBA8 texture that tiles both ways, its texels sharp (or smooth by the setting).
    pub fn texture_tiled(&self, width: u32, height: u32, rgba: &[u8]) -> Arc<Texture> {
        self.upload(width, height, rgba, [&self.sampler_tiled[0], &self.sampler_tiled[1], &self.sampler_aniso[1]])
    }

    fn upload(&self, width: u32, height: u32, rgba: &[u8], [sharp, smooth, filtered]: [&wgpu::Sampler; 3]) -> Arc<Texture> {
        let size = wgpu::Extent3d { width, height, depth_or_array_layers: 1 };
        // (the levels by the setting when it is made; one for a texel)
        let levels = if self.settings.mipmaps { MIPS.min(32 - width.max(height).leading_zeros()) } else { 1 };
        let tex = self.device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size,
            mip_level_count: levels,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo { texture: &tex, mip_level: 0, origin: Default::default(), aspect: Default::default() },
            rgba,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(4 * width), rows_per_image: Some(height) },
            size,
        );
        // (sharp and smooth: the texels alone, as the original's)
        let view = tex.create_view(&wgpu::TextureViewDescriptor { mip_level_count: Some(1), ..Default::default() });
        let bind_group = self.bind(&view, sharp);
        let bind_group_smooth = self.bind(&view, smooth);
        let bind_group_filtered = self.bind(&tex.create_view(&Default::default()), filtered);
        let texels = (levels > 1).then(|| std::sync::Mutex::new(rgba.to_vec()));
        let texture = Texture { bind_group, bind_group_smooth, bind_group_filtered, texture: tex, texels };
        self.write_mips(&texture, rgba);
        Arc::new(texture)
    }

    /// A mipmapped texture's levels from its texels ([`halve`]).
    fn write_mips(&self, texture: &Texture, rgba: &[u8]) {
        let (mut w, mut h) = (texture.texture.width(), texture.texture.height());
        let mut px = rgba.to_vec();
        for level in 1..texture.texture.mip_level_count() {
            let Some((nw, nh, out)) = halve(w, h, &px) else { return };
            self.queue.write_texture(
                wgpu::TexelCopyTextureInfo { texture: &texture.texture, mip_level: level, origin: Default::default(), aspect: Default::default() },
                &out,
                wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(4 * nw), rows_per_image: Some(nh) },
                wgpu::Extent3d { width: nw, height: nh, depth_or_array_layers: 1 },
            );
            (w, h, px) = (nw, nh, out);
        }
    }

    /// Upload an RGBA8 texture for linear filtering with mipmaps (box-filtered on the CPU), tiling both ways.
    /// Transparent texels keep their alpha; the mip levels average the opaque texels' colours so cut-outs do not
    /// darken at their edges.
    pub fn texture_filtered(&self, width: u32, height: u32, rgba: &[u8]) -> Arc<Texture> {
        let mut levels = vec![(width, height, rgba.to_vec())];
        while let Some((w, h, px)) = levels.last().filter(|l| l.0 > 1 || l.1 > 1) {
            let (nw, nh) = ((w / 2).max(1), (h / 2).max(1));
            let mut out = vec![0u8; (nw * nh * 4) as usize];
            for y in 0..nh {
                for x in 0..nw {
                    let (mut c, mut a, mut n) = ([0u32; 3], 0u32, 0u32);
                    for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                        let (sx, sy) = ((2 * x + dx).min(w - 1), (2 * y + dy).min(h - 1));
                        let i = ((sy * w + sx) * 4) as usize;
                        a += px[i + 3] as u32;
                        if px[i + 3] >= 128 {
                            n += 1;
                            for k in 0..3 {
                                c[k] += px[i + k] as u32;
                            }
                        }
                    }
                    let o = ((y * nw + x) * 4) as usize;
                    for k in 0..3 {
                        out[o + k] = c[k].checked_div(n).unwrap_or(0) as u8;
                    }
                    out[o + 3] = (a / 4) as u8;
                }
            }
            levels.push((nw, nh, out));
        }
        let size = wgpu::Extent3d { width, height, depth_or_array_layers: 1 };
        let tex = self.device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size,
            mip_level_count: levels.len() as u32,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        for (level, (w, h, px)) in levels.iter().enumerate() {
            self.queue.write_texture(
                wgpu::TexelCopyTextureInfo { texture: &tex, mip_level: level as u32, origin: Default::default(), aspect: Default::default() },
                px,
                wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(4 * w), rows_per_image: Some(*h) },
                wgpu::Extent3d { width: *w, height: *h, depth_or_array_layers: 1 },
            );
        }
        let view = tex.create_view(&Default::default());
        let bind_group = self.bind(&view, &self.sampler_filtered);
        let bind_group_smooth = self.bind(&view, &self.sampler_filtered);
        let bind_group_filtered = self.bind(&view, &self.sampler_filtered);
        Arc::new(Texture { bind_group, bind_group_smooth, bind_group_filtered, texture: tex, texels: None })
    }

    /// Replace the texels of rectangle (x, y, w, h) of `texture` with `rgba` (RGBA8 rows): the face animation's
    /// copies.
    pub fn update_texture(&self, texture: &Texture, [x, y, w, h]: [u32; 4], rgba: &[u8]) {
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo { texture: &texture.texture, mip_level: 0, origin: wgpu::Origin3d { x, y, z: 0 }, aspect: Default::default() },
            rgba,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(4 * w), rows_per_image: Some(h) },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
        // mipmapped: the rectangle into its texels, the levels made again
        let Some(Ok(mut texels)) = texture.texels.as_ref().map(|t| t.lock()) else { return };
        let tw = texture.texture.width() as usize;
        for (row, line) in rgba.chunks_exact(4 * w as usize).take(h as usize).enumerate() {
            let at = ((y as usize + row) * tw + x as usize) * 4;
            if let Some(dest) = texels.get_mut(at..at + line.len()) {
                dest.copy_from_slice(line);
            }
        }
        self.write_mips(texture, &texels);
    }

    /// A character's GPU model: `indices` from `blend_from` on are its blended primitives'.
    pub fn skinned(&self, vertices: &[SkinVertex], indices: &[u32], blend_from: usize, texture: Arc<Texture>) -> SkinnedModel {
        let vbuf = self.buffer(bytemuck::cast_slice(vertices), wgpu::BufferUsages::VERTEX);
        let ibuf = self.buffer(bytemuck::cast_slice(indices), wgpu::BufferUsages::INDEX);
        let bones = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("bones"),
            size: (MAX_BONES * 64) as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bones_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("bones"),
            layout: &self.bones_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: bones.as_entire_binding() }],
        });
        let count = indices.len() as u32;
        SkinnedModel { vbuf, ibuf, count, blend_from: (blend_from as u32).min(count), bones, bones_group, texture, shadow: None }
    }

    /// Give a model its shadow mesh (skinned to the model's joints).
    pub fn set_shadow(&self, model: &mut SkinnedModel, vertices: &[SkinVertex], indices: &[u32]) {
        let v = self.buffer(bytemuck::cast_slice(vertices), wgpu::BufferUsages::VERTEX);
        let i = self.buffer(bytemuck::cast_slice(indices), wgpu::BufferUsages::INDEX);
        model.shadow = (!indices.is_empty()).then_some((v, i, indices.len() as u32));
    }

    /// The next frame's blob shadows (Force, 0x800793B8): triangles on the floor (metres), each vertex's
    /// colour the amount (of 1) taken off the floor's displayed colour.
    pub fn set_blob_shadows(&mut self, vertices: Vec<StaticVertex>) {
        self.blobs = vertices;
    }

    /// Skinning matrices (joint world × inverse bind, including the fighter's placement), index = joint.
    pub fn update_bones(&self, model: &SkinnedModel, bones: &[Mat4]) {
        let n = bones.len().min(MAX_BONES);
        self.queue.write_buffer(&model.bones, 0, bytemuck::cast_slice(&bones[..n]));
    }

    pub fn static_mesh(&self, vertices: &[StaticVertex], indices: &[u32], texture: Arc<Texture>) -> StaticMesh {
        StaticMesh {
            vbuf: self.buffer(bytemuck::cast_slice(vertices), wgpu::BufferUsages::VERTEX),
            ibuf: self.buffer(bytemuck::cast_slice(indices), wgpu::BufferUsages::INDEX),
            count: indices.len() as u32,
            texture,
            sorted: None,
            blend: 0,
        }
    }

    /// A mesh drawn without depth whose triangles are put in order each frame, the farthest first (the original
    /// hardware's ordering table, by each polygon's average depth): the backdrop drum, whose panels overlap.
    pub fn sorted_mesh(&self, vertices: &[StaticVertex], indices: &[u32], texture: Arc<Texture>) -> StaticMesh {
        let positions = vertices.iter().map(|v| Vec3::from_array(v.pos)).collect();
        let tris = indices.as_chunks::<3>().0.to_vec();
        StaticMesh {
            vbuf: self.buffer(bytemuck::cast_slice(vertices), wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST),
            ibuf: self.buffer(bytemuck::cast_slice(indices), wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST),
            count: indices.len() as u32,
            texture,
            sorted: Some((positions, tris)),
            blend: 0,
        }
    }

    /// A sorted mesh's vertices moved (as many as it was made with: a stage's turning part).
    pub fn move_mesh(&self, m: &mut StaticMesh, vertices: &[StaticVertex]) {
        let Some((positions, _)) = m.sorted.as_mut().filter(|s| s.0.len() == vertices.len()) else { return };
        *positions = vertices.iter().map(|v| Vec3::from_array(v.pos)).collect();
        self.queue.write_buffer(&m.vbuf, 0, bytemuck::cast_slice(vertices));
    }

    /// A sorted mesh's triangles far to near under `view_proj` (stable: ties keep their order).
    fn sort_mesh(&self, m: &StaticMesh, view_proj: Mat4, cull: bool) {
        let Some((pos, tris)) = &m.sorted else { return };
        let clip: Vec<glam::Vec4> = pos.iter().map(|p| view_proj * p.extend(1.0)).collect();
        // the original hardware's NCLIP: a panel seen from behind (reversed on screen) is not drawn, so from outside
        // the drum its near side does not cover the far one
        let facing = |t: &[u32; 3]| {
            let [a, b, c] = t.map(|i| clip[i as usize]);
            if !cull || a.w <= 0.0 || b.w <= 0.0 || c.w <= 0.0 {
                return true;
            }
            let (a, b, c) = (a.truncate().truncate() / a.w, b.truncate().truncate() / b.w, c.truncate().truncate() / c.w);
            (b - a).perp_dot(c - a) >= 0.0
        };
        let mut order: Vec<(f32, [u32; 3])> = tris.iter().filter(|t| facing(t)).map(|t| (t.iter().map(|&i| clip[i as usize].w).sum::<f32>(), *t)).collect();
        order.sort_by(|a, b| b.0.total_cmp(&a.0));
        let mut indices: Vec<u32> = order.into_iter().flat_map(|(_, t)| t).collect();
        // (culled ones: degenerate triangles in their places)
        indices.resize(m.count as usize, 0);
        self.queue.write_buffer(&m.ibuf, 0, bytemuck::cast_slice(&indices));
    }

    fn buffer(&self, data: &[u8], usage: wgpu::BufferUsages) -> wgpu::Buffer {
        self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: None, contents: data, usage })
    }

    /// Draw one frame: sky meshes first (camera-centred, no depth), then static meshes, the overlay's "under"
    /// part, then characters, particles and the debug `lines`; then the `overlay`, and present.
    #[allow(clippy::too_many_arguments)] // one frame's independent parts
    pub fn render_with(
        &mut self,
        cam: &Camera,
        sky: &[&StaticMesh],
        statics: &[&StaticMesh],
        models: &[&SkinnedModel],
        particles: Option<&Particles>,
        lines: &[LineVertex],
        mut overlay: Option<&mut dyn Overlay>,
    ) -> Result<RenderStats> {
        let mut stats = RenderStats::default();
        let proj = cam.projection(self.aspect());
        let view = cam.view();
        let sky_view_proj = proj * Mat4::from_scale(Vec3::new(1.0, cam.sky_y_scale, 1.0)) * view * cam.sky;
        // the drum's panels in the order the game's ordering table draws them: far first
        for m in sky {
            self.sort_mesh(m, sky_view_proj, cam.sky_cull);
        }
        // Tekken Tag's shadows: their darkening, when there are some to draw
        let map_shadow = cam.shadow.filter(|s| s.toward.y > 0.05).and_then(|s| s.alpha);
        // (Tekken Tag's: in the frame's sRGB bytes, where it has a plain view)
        let gamma = cam.gamma && !self.overlay_format().is_srgb();
        let g = Globals {
            view_proj: (proj * view).to_cols_array_2d(),
            sky_view_proj: sky_view_proj.to_cols_array_2d(),
            floor: cam.floor.to_cols_array_2d(),
            hit_colour: cam.hit_light.map_or([0.0; 4], |(c, _)| [c[0], c[1], c[2], 0.0]),
            hit_pos: cam.hit_light.map_or([0.0; 4], |(_, p)| [p.x, p.y, p.z, 1.0]),
            light_dir: cam.light.map_or(Vec3::new(0.35, 0.8, 0.5), |l| l.toward).normalize().extend(0.0).to_array(),
            floor_clip: cam.floor_clip.unwrap_or([0.0; 4]),
            floor_light: cam.floor_light.map_or([0.0; 4], |l| {
                [
                    if l.mode == 0 {
                        if l.shade != 0.0 {
                            1.0
                        } else {
                            0.0
                        }
                    } else {
                        l.mode as f32 + 1.0
                    },
                    l.shade,
                    0.0,
                    0.0,
                ]
            }),
            fighters: cam.floor_light.map_or([0.0; 4], |l| [l.fighters[0][0], l.fighters[0][1], l.fighters[1][0], l.fighters[1][1]]),
            eye: cam.eye.extend(1.0).to_array(),
            forward: (cam.target - cam.eye).normalize_or_zero().extend(0.0).to_array(),
            fade: cam.floor_light.map_or([[0.0; 4]; 8], |l| std::array::from_fn(|i| std::array::from_fn(|k| l.table[i * 4 + k]))),
            cutaway: cam.cutaway.map_or([0.0; 4], |(p, m)| [p[0].x, p[0].y, p[0].z, m]),
            cutaway2: cam.cutaway.map_or([0.0; 4], |(p, _)| [p[1].x, p[1].y, p[1].z, f32::from(u8::from(cam.cutaway_all))]),
            near_cut: cam.near_cut.map_or([0.0; 4], |z| [z, 1.0, 0.0, 0.0]),
            key_color: cam.light.map_or([0.0; 4], |l| [l.key.x, l.key.y, l.key.z, if l.more.is_some() { 2.0 } else { 1.0 }]),
            ambient: cam.light.map_or([0.0; 4], |l| l.ambient.extend(0.0).to_array()),
            material: cam.light.map_or([0.0; 4], |l| l.material.extend(0.0).to_array()),
            shadow: cam.shadow.filter(|s| s.toward.y > 0.05).map_or([0.0; 4], |s| [s.toward.x / s.toward.y, s.toward.z / s.toward.y, s.floor, 1.0]),
            shadow_map: map_shadow.map_or([0.0; 4], |a| [cam.target.x, cam.target.z, 1.0 / SHADOW_HALF, a]),
            floor_levels: cam.floor_levels.map_or([0.0; 4], |l| l.limits),
            floor_cell: cam.floor_levels.map_or([0.0; 4], |l| [l.half[0], l.half[1], 1.0, l.reach]),
            floor_window: cam.floor_levels.map_or([0.0; 4], |l| [l.window[0], l.window[1], l.cell[0], l.cell[1]]),
            floor_centre: cam.floor_levels.map_or([0.0; 4], |l| [l.centre[0], l.centre[1], proj.y_axis.y / proj.x_axis.x, 0.0]),
            shadow_colour: cam.shadow.map_or([0.0; 4], |s| {
                // sRGB to the frame's linear colour (as it is on its sRGB bytes)
                let lin = |v: f32| {
                    if gamma {
                        v
                    } else if v <= 0.04045 {
                        v / 12.92
                    } else {
                        ((v + 0.055) / 1.055).powf(2.4)
                    }
                };
                [lin(s.colour.x), lin(s.colour.y), lin(s.colour.z), 1.0]
            }),
            lights: cam.light.and_then(|l| l.more).map_or([[0.0; 4]; 4], |[(c1, l1), (c2, l2)]| {
                [l1.normalize_or_zero().extend(0.0).to_array(), c1.extend(0.0).to_array(), l2.normalize_or_zero().extend(0.0).to_array(), c2.extend(0.0).to_array()]
            }),
            sky_eye: cam.sky.inverse().transform_point3(cam.eye).extend(1.0).to_array(),
            view_x: view.row(0).to_array(),
            view_y: view.row(1).to_array(),
            fog_colour: cam.fog.map_or([0.0; 4], |(c, _)| {
                let lin = |v: f32| {
                    if gamma || v <= 0.04045 {
                        if gamma {
                            v
                        } else {
                            v / 12.92
                        }
                    } else {
                        ((v + 0.055) / 1.055).powf(2.4)
                    }
                };
                [lin(c[0]), lin(c[1]), lin(c[2]), 1.0]
            }),
            gamma: [f32::from(u8::from(gamma)), 0.0, 0.0, 0.0],
            grass_grid: cam.grass.map_or([0.0; 4], |l| [l.first[0], l.first[1], l.cell[0], l.cell[1]]),
            grass_info: cam.grass.map_or([0.0; 4], |l| [l.row as f32, l.rows as f32, l.size, 1.0]),
            grass: cam.grass.map_or([[0.0; 4]; GRASS], |l| l.moves),
            fog_line: cam.fog.map_or([0.0; 4], |(_, l)| [l[0], l[1], 0.0, 1.0]),
            enhance: [f32::from(u8::from(self.settings.soft_shadows)), 0.0, 0.0, 0.0],
        };
        self.queue.write_buffer(&self.globals, 0, bytemuck::bytes_of(&g));

        let frame = match &self.surface {
            Some(surface) => match surface.get_current_texture() {
                wgpu::CurrentSurfaceTexture::Success(f) | wgpu::CurrentSurfaceTexture::Suboptimal(f) => Some(f),
                wgpu::CurrentSurfaceTexture::Occluded => return Ok(RenderStats { skipped: 1, ..stats }), // window hidden: skip the frame
                other => {
                    log::warn!("surface: {other:?}; reconfiguring");
                    surface.configure(&self.device, &self.config);
                    return Ok(stats);
                }
            },
            None => None,
        };
        let frame_view = match &frame {
            Some(f) => f.texture.create_view(&wgpu::TextureViewDescriptor { format: Some(self.frame_format), ..Default::default() }),
            None => self.offscreen.as_ref().context("no render target")?.create_view(&Default::default()),
        };
        // the 3D (and the HUD under the fighters) into the scene's target at the render scale, or the frame
        let overlay_format = self.overlay_format();
        // (the texture filter: 0 sharp, 1 smooth, 2 filtered)
        let smooth = if self.settings.mipmaps { 2 } else { usize::from(self.settings.smooth_textures) };
        let scene = self.scene_target(if gamma { overlay_format } else { self.frame_format }).map(|t| {
            let ov = wgpu::TextureViewDescriptor { format: Some(overlay_format), ..Default::default() };
            (t.colour.create_view(&Default::default()), t.colour.create_view(&ov), t.depth.clone(), t.size)
        });
        // anti-aliased: the passes draw into the samples, resolved into the scene's target
        let msaa = self.scene.as_ref().and_then(|t| t.msaa.as_ref().map(|m| (m.2.clone(), m.3.clone(), t.reload[usize::from(gamma)].clone())));
        // the glow's pipelines and targets, wanted with something added to glow
        let glows = self.settings.glow && (particles.is_some_and(|p| !p.additive.is_empty() || !p.soft_add.is_empty() || !p.flat_add.is_empty()) || statics.iter().chain(sky).any(|m| m.blend == 1));
        if glows {
            let size = scene.as_ref().map_or([self.config.width, self.config.height], |s| s.3).map(|d| (d / 2).max(1));
            self.glow_target(size);
        }
        let pipes = &self.pipes[usize::from(gamma)];
        let view_tex = match &scene {
            Some(s) if gamma => s.1.clone(),
            Some(s) => s.0.clone(),
            None if gamma => {
                let desc = wgpu::TextureViewDescriptor { format: Some(overlay_format), ..Default::default() };
                match &frame {
                    Some(f) => f.texture.create_view(&desc),
                    None => self.offscreen.as_ref().context("no render target")?.create_view(&desc),
                }
            }
            None => frame_view.clone(),
        };
        let depth_view = scene.as_ref().map_or_else(|| self.depth.clone(), |s| s.2.clone());
        let (resolved, view_tex, depth_view) = match &msaa {
            Some((c, d, _)) => (Some(view_tex), c.clone(), d.clone()),
            None => (None, view_tex, depth_view),
        };
        let line_buf = (!lines.is_empty()).then(|| self.buffer(bytemuck::cast_slice(lines), wgpu::BufferUsages::VERTEX));
        let blobs = std::mem::take(&mut self.blobs);
        let blob_buf = (!blobs.is_empty()).then(|| (self.buffer(bytemuck::cast_slice(&blobs), wgpu::BufferUsages::VERTEX), blobs.len() as u32));
        let flat_bufs: Vec<(&wgpu::RenderPipeline, wgpu::Buffer, u32)> = particles
            .map(|p| {
                [(&pipes.particle_alpha, p.flat_alpha), (&pipes.particle_add, p.flat_add)]
                    .into_iter()
                    .filter(|(_, v)| !v.is_empty())
                    .map(|(pipe, v)| (pipe, self.buffer(bytemuck::cast_slice(v), wgpu::BufferUsages::VERTEX), v.len() as u32))
                    .collect()
            })
            .unwrap_or_default();
        let particle_bufs: Vec<(&wgpu::RenderPipeline, wgpu::Buffer, u32)> = particles
            .map(|p| {
                [(&pipes.particle_alpha, p.alpha), (&pipes.particle_add, p.additive), (&pipes.soft_alpha, p.soft_alpha), (&pipes.soft_add, p.soft_add)]
                    .into_iter()
                    .filter(|(_, v)| !v.is_empty())
                    .map(|(pipe, v)| (pipe, self.buffer(bytemuck::cast_slice(v), wgpu::BufferUsages::VERTEX), v.len() as u32))
                    .collect()
            })
            .unwrap_or_default();
        let overlay_view = || -> Result<wgpu::TextureView> {
            let desc = wgpu::TextureViewDescriptor { format: Some(self.overlay_format()), ..Default::default() };
            Ok(match &frame {
                Some(f) => f.texture.create_view(&desc),
                None => self.offscreen.as_ref().context("no render target")?.create_view(&desc),
            })
        };
        let mut enc = self.device.create_command_encoder(&Default::default());
        // the stage's fill (sRGB) in the frame's linear colour, else the default
        let fill = cam.background.map_or(self.clear, |c| {
            let lin = |v: u8| {
                let v = v as f64 / 255.0;
                if v <= 0.04045 {
                    v / 12.92
                } else {
                    ((v + 0.055) / 1.055).powf(2.4)
                }
            };
            let byte = |v: u8| v as f64 / 255.0;
            if gamma {
                return wgpu::Color { r: byte(c[0]), g: byte(c[1]), b: byte(c[2]), a: 1.0 };
            }
            wgpu::Color { r: lin(c[0]), g: lin(c[1]), b: lin(c[2]), a: 1.0 }
        });
        let pass_desc = |label, clear: bool| {
            let colour = if clear { wgpu::LoadOp::Clear(fill) } else { wgpu::LoadOp::Load };
            let depth = if clear { wgpu::LoadOp::Clear(1.0) } else { wgpu::LoadOp::Load };
            (label, colour, depth)
        };
        let begin = |enc: &mut wgpu::CommandEncoder, (label, colour, depth): (&'static str, wgpu::LoadOp<wgpu::Color>, wgpu::LoadOp<f32>)| {
            enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some(label),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view_tex,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: colour, store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &depth_view,
                    depth_ops: Some(wgpu::Operations { load: depth, store: wgpu::StoreOp::Store }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            })
            .forget_lifetime()
        };
        // the samples resolved into the scene's target
        let resolve = |enc: &mut wgpu::CommandEncoder| {
            let Some(target) = &resolved else { return };
            enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("resolve"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view_tex,
                    depth_slice: None,
                    resolve_target: Some(target),
                    ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
        };
        // (a drum drawn first is no model over the floor: a wall, a drum around the camera)
        let sky_depth = cam.sky_depth && !cam.sky_first;
        {
            let mut pass = begin(&mut enc, pass_desc("stage", true));
            pass.set_bind_group(0, &self.globals_group, &[]);
            // the 2D pictures behind everything, their 4:3 picture's x brought to the window's
            let squeeze = (4.0 / 3.0) / self.aspect();
            for (texture, vertices) in &self.backdrop {
                let vertices: Vec<StaticVertex> = vertices.iter().map(|v| StaticVertex { pos: [v.pos[0] * squeeze, v.pos[1], 0.0], ..*v }).collect();
                let vbuf = self.buffer(bytemuck::cast_slice(&vertices), wgpu::BufferUsages::VERTEX);
                pass.set_pipeline(&pipes.backdrop_pipeline);
                pass.set_bind_group(1, texture.group(smooth), &[]);
                pass.set_vertex_buffer(0, vbuf.slice(..));
                pass.draw(0..vertices.len() as u32, 0..1);
                stats.draw_calls += 1;
            }
            // the drum goes after the floor without depth (the game's OT 1010 vs 1021), or first on the stage
            // stages.json names
            let order = if cam.sky_first { [(&pipes.sky_pipeline, sky), (&pipes.static_pipeline, statics)] } else { [(&pipes.static_pipeline, statics), (&pipes.sky_pipeline, sky)] };
            for (pipeline, meshes) in order.into_iter().filter(|o| !(sky_depth && std::ptr::eq(o.0, &pipes.sky_pipeline))) {
                pass.set_pipeline(pipeline);
                for m in meshes.iter().filter(|m| m.blend == 0) {
                    pass.set_bind_group(1, m.texture.group(smooth), &[]);
                    pass.set_vertex_buffer(0, m.vbuf.slice(..));
                    pass.set_index_buffer(m.ibuf.slice(..), wgpu::IndexFormat::Uint32);
                    pass.draw_indexed(0..m.count, 0, 0..1);
                    stats.draw_calls += 1;
                    stats.triangles += m.count / 3;
                }
            }
        }
        // a model with depth: over the floor with a depth buffer of its own; then the floor's depth again, for the
        // fighters and their shadows
        if sky_depth {
            let passes = [("stage model", &pipes.sky_depth_pipeline, sky), ("floor depth", &pipes.static_depth_pipeline, statics)];
            for (label, pipeline, meshes) in passes.into_iter().take(if cam.sky_floor_depth { 1 } else { 2 }) {
                let depth = if cam.sky_floor_depth { wgpu::LoadOp::Load } else { wgpu::LoadOp::Clear(1.0) };
                let mut pass = begin(&mut enc, (label, wgpu::LoadOp::Load, depth));
                pass.set_bind_group(0, &self.globals_group, &[]);
                pass.set_pipeline(pipeline);
                for m in meshes.iter().filter(|m| m.blend == 0) {
                    pass.set_bind_group(1, m.texture.group(smooth), &[]);
                    pass.set_vertex_buffer(0, m.vbuf.slice(..));
                    pass.set_index_buffer(m.ibuf.slice(..), wgpu::IndexFormat::Uint32);
                    pass.draw_indexed(0..m.count, 0, 0..1);
                    stats.draw_calls += 1;
                    // (the floor's triangles are counted where its colour is drawn)
                    if std::ptr::eq(pipeline, &pipes.sky_depth_pipeline) {
                        stats.triangles += m.count / 3;
                    }
                }
            }
        }
        // the stage's added and multiplying strips (Tekken Tag's), over the stage it lights or shades
        let strips: Vec<(usize, &StaticMesh)> = statics.iter().map(|m| (0, *m)).chain(sky.iter().map(|m| (2, *m))).filter(|(_, m)| m.blend > 0).collect();
        if !strips.is_empty() {
            let mut pass = begin(&mut enc, ("stage strips", wgpu::LoadOp::Load, wgpu::LoadOp::Load));
            pass.set_bind_group(0, &self.globals_group, &[]);
            for (base, m) in strips {
                let Some(pipeline) = pipes.stage_blends.get(base + usize::from(m.blend) - 1) else { continue };
                pass.set_pipeline(pipeline);
                pass.set_bind_group(1, m.texture.group(smooth), &[]);
                pass.set_vertex_buffer(0, m.vbuf.slice(..));
                pass.set_index_buffer(m.ibuf.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..m.count, 0, 0..1);
                stats.draw_calls += 1;
                stats.triangles += m.count / 3;
            }
        }
        // the 2D drawn behind the fighters, then the fighters over it
        if let Some(o) = overlay.as_deref_mut() {
            resolve(&mut enc);
            match &scene {
                Some((_, ov, _, size)) => o.draw_under(&self.device, &self.queue, &mut enc, ov, *size, size[0] as f32 / self.config.width as f32),
                None => o.draw_under(&self.device, &self.queue, &mut enc, &overlay_view()?, [self.config.width, self.config.height], 1.0),
            }
            if let Some((_, _, group)) = &msaa {
                let mut pass = begin(&mut enc, ("reload", wgpu::LoadOp::Load, wgpu::LoadOp::Load));
                pass.set_pipeline(&pipes.reload);
                pass.set_bind_group(0, group, &[]);
                pass.draw(0..3, 0..1);
            }
        }
        if map_shadow.is_some() {
            let view = self.shadow_map.create_view(&Default::default());
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("shadow picture"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_bind_group(0, &self.globals_group, &[]);
            // (four fighters at most: the tag brawl's)
            for (m, pipeline) in models.iter().filter(|m| m.shadow.is_some()).zip(&self.shadow_map_pipelines) {
                let Some((vbuf, ibuf, count)) = &m.shadow else { continue };
                pass.set_pipeline(pipeline);
                pass.set_bind_group(1, m.texture.group(smooth), &[]);
                pass.set_bind_group(2, &m.bones_group, &[]);
                pass.set_vertex_buffer(0, vbuf.slice(..));
                pass.set_index_buffer(ibuf.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..*count, 0, 0..1);
                stats.draw_calls += 1;
            }
        }
        {
            let mut pass = begin(&mut enc, pass_desc("characters", false));
            pass.set_bind_group(0, &self.globals_group, &[]);
            // the shadows first, on the floor under the fighters
            if map_shadow.is_some() {
                // Tekken Tag's: the floor darkened under the shadow picture
                pass.set_pipeline(&pipes.floor_shadow_pipeline);
                pass.set_bind_group(1, &self.shadow_map_group, &[]);
                for m in statics {
                    pass.set_vertex_buffer(0, m.vbuf.slice(..));
                    pass.set_index_buffer(m.ibuf.slice(..), wgpu::IndexFormat::Uint32);
                    pass.draw_indexed(0..m.count, 0, 0..1);
                    stats.draw_calls += 1;
                }
            } else if cam.shadow.is_some_and(|s| s.toward.y > 0.05) {
                pass.set_pipeline(&pipes.shadow_pipeline);
                for m in models {
                    let Some((vbuf, ibuf, count)) = &m.shadow else { continue };
                    pass.set_bind_group(1, m.texture.group(smooth), &[]);
                    pass.set_bind_group(2, &m.bones_group, &[]);
                    pass.set_vertex_buffer(0, vbuf.slice(..));
                    pass.set_index_buffer(ibuf.slice(..), wgpu::IndexFormat::Uint32);
                    pass.draw_indexed(0..*count, 0, 0..1);
                    stats.draw_calls += 1;
                }
            }
            if let (Some((buf, n)), Some(white)) = (&blob_buf, self.white.as_ref()) {
                pass.set_pipeline(&pipes.blob_pipeline);
                pass.set_bind_group(1, &white.bind_group, &[]);
                pass.set_vertex_buffer(0, buf.slice(..));
                pass.draw(0..*n, 0..1);
                stats.draw_calls += 1;
            }
            pass.set_pipeline(&pipes.skinned_pipeline);
            for m in models {
                pass.set_bind_group(1, m.texture.group(smooth), &[]);
                pass.set_bind_group(2, &m.bones_group, &[]);
                pass.set_vertex_buffer(0, m.vbuf.slice(..));
                pass.set_index_buffer(m.ibuf.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..m.blend_from, 0, 0..1);
                stats.draw_calls += 1;
                stats.triangles += m.count / 3;
            }
            pass.set_pipeline(&pipes.skinned_blend_pipeline);
            for m in models.iter().filter(|m| m.blend_from < m.count) {
                pass.set_bind_group(1, m.texture.group(smooth), &[]);
                pass.set_bind_group(2, &m.bones_group, &[]);
                pass.set_vertex_buffer(0, m.vbuf.slice(..));
                pass.set_index_buffer(m.ibuf.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(m.blend_from..m.count, 0, 0..1);
                stats.draw_calls += 1;
            }
            if let (false, Some(white)) = (flat_bufs.is_empty(), self.white.as_ref()) {
                pass.set_bind_group(1, &white.bind_group, &[]);
                for (pipe, buf, n) in &flat_bufs {
                    pass.set_pipeline(pipe);
                    pass.set_vertex_buffer(0, buf.slice(..));
                    pass.draw(0..*n, 0..1);
                    stats.draw_calls += 1;
                    stats.triangles += n / 3;
                }
            }
            if let Some(p) = particles {
                pass.set_bind_group(1, &p.texture.bind_group, &[]);
                for (pipe, buf, n) in &particle_bufs {
                    pass.set_pipeline(pipe);
                    pass.set_vertex_buffer(0, buf.slice(..));
                    pass.draw(0..*n, 0..1);
                    stats.draw_calls += 1;
                    stats.triangles += n / 3;
                }
            }
            if let (Some(c), Some(white)) = (self.negative, self.white.as_ref()) {
                let v = |x: f32, y: f32| StaticVertex { pos: [x, y, 0.0], uv: [0.0; 2], color: [c[0], c[1], c[2], 1.0], cell: [0.0; 4], normal: [0.0; 4] };
                let vbuf = self.buffer(bytemuck::cast_slice(&[v(-1.0, -1.0), v(3.0, -1.0), v(-1.0, 3.0)]), wgpu::BufferUsages::VERTEX);
                pass.set_pipeline(&pipes.negative_pipeline);
                pass.set_bind_group(1, &white.bind_group, &[]);
                pass.set_vertex_buffer(0, vbuf.slice(..));
                pass.draw(0..3, 0..1);
                stats.draw_calls += 1;
            }
            if let Some(buf) = &line_buf {
                pass.set_pipeline(&pipes.line_pipeline);
                pass.set_vertex_buffer(0, buf.slice(..));
                pass.draw(0..lines.len() as u32, 0..1);
                stats.draw_calls += 1;
                stats.lines = lines.len() as u32 / 2;
            }
        }
        resolve(&mut enc);
        // the glow: the added lights drawn again into its picture at half size (behind the stage and the fighters,
        // drawn first), blurred, added to the scene
        if let (true, Some(gp), Some(gl)) = (glows, self.glow_pipes.as_ref(), self.glow.as_ref()) {
            let target = |enc: &mut wgpu::CommandEncoder, label, view: &wgpu::TextureView, depth: Option<wgpu::LoadOp<f32>>, colour: wgpu::LoadOp<wgpu::Color>| {
                enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some(label),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment { view, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: colour, store: wgpu::StoreOp::Store } })],
                    depth_stencil_attachment: depth.map(|load| wgpu::RenderPassDepthStencilAttachment {
                        view: &gl.depth,
                        depth_ops: Some(wgpu::Operations { load, store: wgpu::StoreOp::Store }),
                        stencil_ops: None,
                    }),
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                })
                .forget_lifetime()
            };
            let black = wgpu::LoadOp::Clear(wgpu::Color::BLACK);
            {
                let mut pass = target(&mut enc, "glow depth", &gl.views[0], Some(wgpu::LoadOp::Clear(1.0)), black);
                pass.set_bind_group(0, &self.globals_group, &[]);
                pass.set_pipeline(&gp.static_pipeline);
                for m in statics.iter().filter(|m| m.blend == 0) {
                    pass.set_bind_group(1, m.texture.group(smooth), &[]);
                    pass.set_vertex_buffer(0, m.vbuf.slice(..));
                    pass.set_index_buffer(m.ibuf.slice(..), wgpu::IndexFormat::Uint32);
                    pass.draw_indexed(0..m.count, 0, 0..1);
                }
                pass.set_pipeline(&gp.skinned_pipeline);
                for m in models {
                    pass.set_bind_group(1, m.texture.group(smooth), &[]);
                    pass.set_bind_group(2, &m.bones_group, &[]);
                    pass.set_vertex_buffer(0, m.vbuf.slice(..));
                    pass.set_index_buffer(m.ibuf.slice(..), wgpu::IndexFormat::Uint32);
                    pass.draw_indexed(0..m.blend_from, 0, 0..1);
                }
            }
            {
                let mut pass = target(&mut enc, "glow", &gl.views[0], Some(wgpu::LoadOp::Load), black);
                pass.set_bind_group(0, &self.globals_group, &[]);
                for (base, m) in statics.iter().map(|m| (0, *m)).chain(sky.iter().map(|m| (2, *m))).filter(|(_, m)| m.blend == 1) {
                    let Some(pipeline) = gp.stage_blends.get(base) else { continue };
                    pass.set_pipeline(pipeline);
                    pass.set_bind_group(1, m.texture.group(smooth), &[]);
                    pass.set_vertex_buffer(0, m.vbuf.slice(..));
                    pass.set_index_buffer(m.ibuf.slice(..), wgpu::IndexFormat::Uint32);
                    pass.draw_indexed(0..m.count, 0, 0..1);
                }
                // (the particles' added kinds, in the glow's pipelines)
                let added = |pipe: &wgpu::RenderPipeline| [(&pipes.particle_add, &gp.particle_add), (&pipes.soft_add, &gp.soft_add)].into_iter().find(|(p, _)| std::ptr::eq(*p, pipe)).map(|(_, g)| g);
                if let Some(white) = self.white.as_ref() {
                    pass.set_bind_group(1, &white.bind_group, &[]);
                    for (pipe, buf, n) in &flat_bufs {
                        let Some(g) = added(pipe) else { continue };
                        pass.set_pipeline(g);
                        pass.set_vertex_buffer(0, buf.slice(..));
                        pass.draw(0..*n, 0..1);
                    }
                }
                if let Some(p) = particles {
                    pass.set_bind_group(1, &p.texture.bind_group, &[]);
                    for (pipe, buf, n) in &particle_bufs {
                        let Some(g) = added(pipe) else { continue };
                        pass.set_pipeline(g);
                        pass.set_vertex_buffer(0, buf.slice(..));
                        pass.draw(0..*n, 0..1);
                    }
                }
            }
            for (k, pipeline) in self.glow_blur.iter().enumerate() {
                let mut pass = target(&mut enc, "glow blur", &gl.views[1 - k], None, black);
                pass.set_pipeline(pipeline);
                pass.set_bind_group(0, &gl.groups[k], &[]);
                pass.draw(0..3, 0..1);
            }
            let mut pass = target(&mut enc, "glow add", resolved.as_ref().unwrap_or(&view_tex), None, wgpu::LoadOp::Load);
            pass.set_pipeline(&self.glow_add[usize::from(gamma)]);
            pass.set_bind_group(0, &gl.groups[0], &[]);
            pass.draw(0..3, 0..1);
        }
        // the scene to the window
        if let (Some(t), Some(_)) = (self.scene.as_ref(), &scene) {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("blit"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &frame_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(self.clear), store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.blit_pipeline);
            pass.set_bind_group(0, &t.group, &[]);
            pass.draw(0..3, 0..1);
        }
        if let Some(o) = overlay {
            o.draw(&self.device, &self.queue, &mut enc, &overlay_view()?, [self.config.width, self.config.height]);
        }
        let readback = match (&frame, self.capture_next) {
            (Some(f), true) if self.config.usage.contains(wgpu::TextureUsages::COPY_SRC) => Some(self.copy_out(&mut enc, &f.texture)),
            (Some(_), true) => {
                self.captured = Some(Err(anyhow::anyhow!("this window's frames cannot be read back")));
                None
            }
            _ => None,
        };
        self.capture_next = false;
        self.queue.submit([enc.finish()]);
        // (shown before the readback: a failed capture does not drop the frame)
        if let Some(f) = frame {
            self.queue.present(f);
        }
        if let Some((buf, padded)) = readback {
            self.captured = Some(self.read_back(&buf, padded));
        }
        Ok(stats)
    }

    /// Queue a copy of `tex` into a mappable buffer (rows padded to 256 bytes).
    fn copy_out(&self, enc: &mut wgpu::CommandEncoder, tex: &wgpu::Texture) -> (wgpu::Buffer, u32) {
        let (w, h) = (self.config.width, self.config.height);
        let padded = (4 * w).div_ceil(256) * 256; // rows must be 256-byte aligned for copies
        let buf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("capture"),
            size: (padded * h) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        enc.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo { texture: tex, mip_level: 0, origin: Default::default(), aspect: Default::default() },
            wgpu::TexelCopyBufferInfo { buffer: &buf, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(padded), rows_per_image: Some(h) } },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
        (buf, padded)
    }

    /// Map a buffer from [`Renderer::copy_out`] and unpack it to RGBA8 (BGRA surfaces are swizzled).
    fn read_back(&self, buf: &wgpu::Buffer, padded: u32) -> Result<Image> {
        let (w, h) = (self.config.width, self.config.height);
        let slice = buf.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        self.device.poll(wgpu::PollType::wait_indefinitely())?;
        let data = slice.get_mapped_range().map_err(|e| anyhow::anyhow!("map: {e:?}"))?;
        let mut rgba = Vec::with_capacity((4 * w * h) as usize);
        for row in 0..h as usize {
            rgba.extend_from_slice(&data[row * padded as usize..row * padded as usize + 4 * w as usize]);
        }
        if matches!(self.config.format, wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb) {
            for px in rgba.as_chunks_mut::<4>().0 {
                px.swap(0, 2);
            }
        }
        Ok(Image { width: w, height: h, rgba })
    }

    /// Read back the last offscreen frame as tightly packed RGBA8 (headless mode only).
    pub fn capture(&self) -> Result<Image> {
        let tex = self.offscreen.as_ref().context("capture needs a headless renderer")?;
        let mut enc = self.device.create_command_encoder(&Default::default());
        let (buf, padded) = self.copy_out(&mut enc, tex);
        self.queue.submit([enc.finish()]);
        self.read_back(&buf, padded)
    }
}

/// The next mip level of RGBA8 texels (None: fewer texels than `w` x `h`): each texel the four above it, their
/// colours weighted by their alpha (a cut-out's transparent texels do not darken its edges), the alpha averaged.
fn halve(w: u32, h: u32, px: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    let (nw, nh) = ((w / 2).max(1), (h / 2).max(1));
    let mut out = vec![0u8; (nw * nh * 4) as usize];
    for (i, o) in out.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        let (x, y) = (i as u32 % nw, i as u32 / nw);
        let (mut c, mut a, mut k) = ([0u32; 3], 0u32, 0u32);
        for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
            let at = (((2 * y + dy).min(h - 1) * w + (2 * x + dx).min(w - 1)) * 4) as usize;
            let t = px.get(at..at + 4)?;
            // (+1: a block all transparent keeps its colours' average)
            let weight = u32::from(t[3]) + 1;
            (a, k) = (a + u32::from(t[3]), k + weight);
            for (c, t) in c.iter_mut().zip(t) {
                *c += u32::from(*t) * weight;
            }
        }
        for (o, c) in o.iter_mut().zip(c) {
            *o = (c / k) as u8;
        }
        o[3] = (a / 4) as u8;
    }
    Some((nw, nh, out))
}

/// Tekken Tag's shadow picture of `side` texels a side, and its bind group.
fn shadow_picture(device: &wgpu::Device, layout: &wgpu::BindGroupLayout, sampler: &wgpu::Sampler, side: u32) -> (wgpu::Texture, wgpu::BindGroup) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("shadow picture"),
        size: wgpu::Extent3d { width: side, height: side, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let view = texture.create_view(&Default::default());
    let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout,
        entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) }, wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(sampler) }],
    });
    (texture, group)
}

fn make_offscreen(device: &wgpu::Device, config: &wgpu::SurfaceConfiguration) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("offscreen"),
        size: wgpu::Extent3d { width: config.width, height: config.height, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: config.format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &config.view_formats,
    })
}

fn make_depth(device: &wgpu::Device, config: &wgpu::SurfaceConfiguration) -> wgpu::TextureView {
    device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some("depth"),
            size: wgpu::Extent3d { width: config.width, height: config.height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        })
        .create_view(&Default::default())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A mip level keeps a cut-out's colour at its edge (the transparent texels' black does not bleed in).
    #[test]
    fn mips_weigh_by_alpha() {
        let px = [200, 100, 50, 255, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        assert_eq!(halve(2, 2, &px), Some((1, 1, vec![197, 98, 49, 63])));
        assert_eq!(halve(2, 2, &px[..8]), None);
    }

    /// A roll turns the picture anticlockwise: what is to the right of the target goes up.
    #[test]
    fn roll_raises_the_right() {
        let cam = Camera { roll: 0.2, ..Camera::new(Vec3::new(0.0, 1.0, 5.0), Vec3::new(0.0, 1.0, 0.0), 0.8) };
        let p = cam.view().transform_point3(Vec3::new(1.0, 1.0, 0.0));
        assert!(p.x > 0.9 && p.y > 0.15 && (p.z + 5.0).abs() < 1e-5, "{p:?}");
    }
}
