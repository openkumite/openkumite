// OpenKumite shaders. Positions are in the converted assets' space (metres, y up).

struct Globals {
    view_proj: mat4x4<f32>,
    sky_view_proj: mat4x4<f32>,
    floor: mat4x4<f32>,
    light_dir: vec4<f32>,
    // the hit light (0x80039F0C): colour × strength, and where it is (metres)
    hit_colour: vec4<f32>,
    hit_pos: vec4<f32>,
    // the stage meshes kept inside x0..x1, z0..z1 (metres) when x0 < x1: the floor's finite patch
    floor_clip: vec4<f32>,
    // the extra stages' floor lighting: x mode (0 none, 1 depth fade, 2 from each fighter, 3 from their midpoint), y shade
    floor_light: vec4<f32>,
    fighters: vec4<f32>,
    eye: vec4<f32>,
    forward: vec4<f32>,
    fade: array<vec4<f32>, 8>,
    // an arena with walls: the fighter nearer the camera (xyz, metres) and the margin (w; 0 = off)
    cutaway: vec4<f32>,
    cutaway2: vec4<f32>,
    // a level laid along a route (Force's extra stages): above the floor, nothing beyond z = x (the camera's
    // side) is drawn (y = 1: on); z: Tekken Tag's scenery not drawn nearer (Camera::scenery_cut)
    near_cut: vec4<f32>,
    // the stage's light for the fighters (w = 1 when there is one): key colour, ambient, material (128 = 1)
    key_color: vec4<f32>,
    ambient: vec4<f32>,
    material: vec4<f32>,
    // the fighters' shadows: the light's x / y and z / y, the floor's height, w = 1 when on; their colour (a: alpha)
    shadow: vec4<f32>,
    shadow_colour: vec4<f32>,
    // Tekken Tag's shadow picture: its centre's x, z (metres), 1 / its half size, the darkening (0: none)
    shadow_map: vec4<f32>,
    // Tekken Tag's floor cells: the depth limits (metres); the half size of a cell's square (x, z), 1 when on, the
    // reach; the window (cells x, z; 0: none) and the cell's size; the window's centre (x, z), the picture's aspect
    floor_levels: vec4<f32>,
    floor_cell: vec4<f32>,
    floor_window: vec4<f32>,
    floor_centre: vec4<f32>,
    // Tekken Tag's two other lights: towards light 1, its colour, towards light 2, its colour
    lights: array<vec4<f32>, 4>,
    // Tekken Tag's arena: the eye in the drum's placement (metres), the camera's x and y axes
    sky_eye: vec4<f32>,
    view_x: vec4<f32>,
    view_y: vec4<f32>,
    // Tekken Tag's arena fog: its colour (linear), the coefficient's line (255 + slope x depth in mm; w = 1 when on)
    fog_colour: vec4<f32>,
    fog_line: vec4<f32>,
    // x 1: the 3D pass on the frame's sRGB bytes (Tekken Tag's: the PS2's GS modulates and blends them)
    gamma: vec4<f32>,
    // Eddy's grass: the first corner (x, z), a cell; corners a row, rows, the lattice's height, 1 when on; the moves
    grass_grid: vec4<f32>,
    grass_info: vec4<f32>,
    grass: array<vec4<f32>, 289>,
    // OpenKumite's enhancements: x 1 soft shadows
    enhance: vec4<f32>,
};
@group(0) @binding(0) var<uniform> g: Globals;
@group(1) @binding(0) var tex: texture_2d<f32>;
@group(1) @binding(1) var samp: sampler;
@group(2) @binding(0) var<uniform> bones: array<mat4x4<f32>, 160>;

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    // (anti-aliased: at a covered sample, so an edge's pixels do not read past its texture's part of the atlas)
    @location(0) @interpolate(perspective, centroid) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) world: vec3<f32>,
    @location(3) normal: vec3<f32>,
    // Tekken Tag's arena: above 0 on a triangle the PS2 does not draw (a corner outside its guard band, below)
    @location(4) dropped: f32,
    // Tekken Tag's arena: how much of the fog colour (0: none)
    @location(5) fog: f32,
    // Tekken Tag's fighter's environment map: its coordinates (repeating) and its size in the texture (zw 0: none)
    @location(6) env: vec4<f32>,
};

// Standard 4-joint linear blend skinning. Tekken Tag's fighters (its three lights) lit per vertex as its VU0
// programs light them (0x3439B0), and their depth pushed back by their bias.
@vertex
fn vs_skinned(@location(0) pos: vec3<f32>, @location(1) uv: vec2<f32>,
              @location(2) joints: vec4<u32>, @location(3) weights: vec4<f32>, @location(4) normal: vec3<f32>,
              @location(5) look: vec4<f32>) -> VsOut {
    let skin = weights.x * bones[joints.x] + weights.y * bones[joints.y]
             + weights.z * bones[joints.z] + weights.w * bones[joints.w];
    let p = (skin * vec4(pos, 1.0)).xyz;
    var o: VsOut;
    o.clip = g.view_proj * vec4(p, 1.0);
    o.uv = uv;
    o.color = vec4(1.0);
    o.world = p;
    o.normal = (skin * vec4(normal, 0.0)).xyz;
    if (g.key_color.w > 1.5) {
        o.color = vec4(ttt_light(o, look), 1.0);
        if (look.y > 1.5 && look.y < 2.5) {
            o.env = vec4(ttt_env(o.normal), look.zw);
        }
        // (the programs divide the depth by w less the bias, the mm the vertex lies farther: along its line of sight)
        if (look.x > 0.0) {
            let b = g.view_proj * vec4(p + normalize(p - g.eye.xyz) * look.x, 1.0);
            o.clip.z = b.z / b.w * o.clip.w;
        }
    }
    return o;
}

// The way to the eye from the fighter's root (VU0 word 11: the eye less player +0x590, normalised).
fn ttt_to_eye() -> vec3<f32> {
    return normalize(g.eye.xyz - bones[0][3].xyz);
}

// A Tekken Tag fighter's vertex colour (128 = 1), by its lighting `look.y`: 0 the ambient and three lights on its
// summed normal (program 0x000, not normalised), 1 and 2 none (the GS's decal), 3 those on the normalised normal and
// light 0's highlight added to light 0 (0x04A), 4 the highlight alone (0x0EA): (R . L0)^(2^n) x strength, R the
// way to the eye reflected about the normal; the hit light as in Tekken 3's. Saturating at 255.
fn ttt_light(o: VsOut, look: vec4<f32>) -> vec3<f32> {
    let mode = look.y;
    if (mode > 0.5 && mode < 2.5) { return vec3(1.0); }
    var n = o.normal;
    var s = 0.0;
    if (mode > 2.5) {
        n = normalize(n);
        let v = ttt_to_eye();
        s = dot(2.0 * n * dot(n, v) - v, g.light_dir.xyz);
        for (var k = 0; k < min(i32(look.w), 32); k++) { s = s * s; }
        s = s * look.z;
    }
    let cap = vec3(255.0 / 128.0);
    if (mode > 3.5) { return min(g.key_color.rgb * s, cap); }
    let hit = g.hit_colour.rgb * max(0.0, dot(n, normalize(g.hit_pos.xyz - o.world + vec3(0.0, 0.0001, 0.0))));
    let l = g.ambient.rgb + g.key_color.rgb * (max(0.0, dot(n, g.light_dir.xyz)) + s)
        + g.lights[1].rgb * max(0.0, dot(n, g.lights[0].xyz)) + g.lights[3].rgb * max(0.0, dot(n, g.lights[2].xyz));
    return min(l, cap) + hit;
}

// A Tekken Tag fighter's environment map's coordinates (program 0x186): half of one plus the camera's x and y (down)
// of the way to the eye plus the normalised normal.
fn ttt_env(normal: vec3<f32>) -> vec2<f32> {
    let h = ttt_to_eye() + normalize(normal);
    return 0.5 * (1.0 + vec2(dot(h, g.view_x.xyz), -dot(h, g.view_y.xyz)));
}

// A fighter's shadow (0x80035F2C's matrix): each vertex moved along the light onto the floor.
@vertex
fn vs_shadow(@location(0) pos: vec3<f32>, @location(1) uv: vec2<f32>,
             @location(2) joints: vec4<u32>, @location(3) weights: vec4<f32>, @location(4) normal: vec3<f32>) -> VsOut {
    return shadow_vertex(pos, uv, joints, weights);
}

// A shadow mesh's vertex skinned and moved along the light onto the floor's plane.
fn shadow_vertex(pos: vec3<f32>, uv: vec2<f32>, joints: vec4<u32>, weights: vec4<f32>) -> VsOut {
    let skin = weights.x * bones[joints.x] + weights.y * bones[joints.y]
             + weights.z * bones[joints.z] + weights.w * bones[joints.w];
    var p = (skin * vec4(pos, 1.0)).xyz;
    let h = p.y - g.shadow.z;
    p = vec3(p.x - h * g.shadow.x, g.shadow.z + 0.003, p.z - h * g.shadow.y);
    var o: VsOut;
    o.clip = g.view_proj * vec4(p, 1.0);
    o.uv = uv;
    o.color = vec4(1.0);
    o.world = p;
    o.normal = vec3(0.0);
    return o;
}

// Tekken Tag's shadow picture (0x342390): a fighter's silhouette moved along the light onto the floor's plane,
// seen from above: x, z about the picture's centre.
@vertex
fn vs_shadow_map(@location(0) pos: vec3<f32>, @location(1) uv: vec2<f32>,
                 @location(2) joints: vec4<u32>, @location(3) weights: vec4<f32>, @location(4) normal: vec3<f32>) -> VsOut {
    var o = shadow_vertex(pos, uv, joints, weights);
    let k = g.shadow_map.z;
    o.clip = vec4((o.world.x - g.shadow_map.x) * k, -(o.world.z - g.shadow_map.y) * k, 0.5, 1.0);
    return o;
}

@fragment
fn fs_shadow_map(i: VsOut) -> @location(0) vec4<f32> {
    return vec4(1.0);
}

// The floor drawn again over itself (0x342128: the game draws it with the shadow picture projected along the light),
// a hair nearer, so it passes where the floor shows.
@vertex
fn vs_floor_shadow(@location(0) pos: vec3<f32>, @location(1) uv: vec2<f32>, @location(2) color: vec4<f32>,
                   @location(3) cell: vec4<f32>, @location(4) normal: vec4<f32>) -> VsOut {
    var o = static_vertex(pos, uv, color, cell, normal);
    o.clip.z = o.clip.z - 0.00002 * o.clip.w;
    return o;
}

// Each fighter's picture where this floor point's line along the light meets the plane: the floor times (1 - the
// darkening) under each (ALPHA 0x46: Cd - Cd x As, As the arena's TA1), in the displayed colours.
@fragment
fn fs_floor_shadow(i: VsOut) -> @location(0) vec4<f32> {
    if (i.dropped > 0.0) { discard; }
    let h = i.world.y - g.shadow.z;
    let p = vec2(i.world.x - h * g.shadow.x, i.world.z - h * g.shadow.y);
    let uv = vec2(0.5 + 0.5 * (p.x - g.shadow_map.x) * g.shadow_map.z, 0.5 + 0.5 * (p.y - g.shadow_map.y) * g.shadow_map.z);
    var f = shadow_at(uv);
    if (g.enhance.x > 0.5) {
        // (OpenKumite's soft shadows: the factor averaged over four filtered taps 3 cm apart)
        let d = 0.5 * 0.015 * g.shadow_map.z;
        f = 0.25 * (shadow_at(uv + vec2(-d, -d)) + shadow_at(uv + vec2(d, -d)) + shadow_at(uv + vec2(-d, d)) + shadow_at(uv + vec2(d, d)));
    }
    return vec4(select(srgb_to_linear(vec3(f)), vec3(f), g.gamma.x > 0.5), 1.0);
}

fn shadow_at(uv: vec2<f32>) -> f32 {
    let c = textureSample(tex, samp, clamp(uv, vec2(0.0), vec2(1.0)));
    let a = g.shadow_map.w * f32(all(uv >= vec2(0.0)) && all(uv <= vec2(1.0)));
    return (1.0 - a * c.r) * (1.0 - a * c.g) * (1.0 - a * c.b) * (1.0 - a * c.a);
}

@fragment
fn fs_shadow(i: VsOut) -> @location(0) vec4<f32> {
    return g.shadow_colour;
}

// World-space triangles (metres), not moved with the floor.
@vertex
fn vs_world(@location(0) pos: vec3<f32>, @location(1) uv: vec2<f32>, @location(2) color: vec4<f32>) -> VsOut {
    var o: VsOut;
    o.clip = g.view_proj * vec4(pos, 1.0);
    o.uv = uv;
    o.color = color;
    o.world = pos;
    o.normal = vec3(0.0);
    return o;
}

// A texel as the pass draws: the frame's linear colour, or (g.gamma) its sRGB bytes
fn texel(uv: vec2<f32>) -> vec4<f32> {
    let t = textureSample(tex, samp, uv);
    if (g.gamma.x < 0.5) { return t; }
    let c = t.rgb;
    return vec4(select(1.055 * pow(c, vec3(1.0 / 2.4)) - 0.055, c * 12.92, c <= vec3(0.0031308)), t.a);
}

// Anti-aliased (alpha to coverage): a cut-out's edge as the coverage, sharpened to a pixel's width; else (the
// original's) cut at half
override coverage: bool = false;
fn cut(a: f32) -> f32 {
    if (coverage) {
        let k = clamp((a - 0.5) / max(fwidth(a), 0.0001) + 0.5, 0.0, 1.0);
        if (k <= 0.0) { discard; }
        return k;
    }
    if (a < 0.5) { discard; }
    return 1.0;
}

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    return select(pow((c + 0.055) / 1.055, vec3(2.4)), c / 12.92, c <= vec3(0.04045));
}

// A blob shadow: the original hardware takes the amount off the displayed colour (subtraction in gamma space). The frame here
// is linear and its colour unknown to the shader, so the factor is that subtraction's for a mid-grey floor,
// multiplied in.
@fragment
fn fs_blob(i: VsOut) -> @location(0) vec4<f32> {
    let grey = 0.5;
    let k = srgb_to_linear(max(vec3(grey) - i.color.rgb, vec3(0.0))) / srgb_to_linear(vec3(grey));
    return vec4(k, 1.0);
}

@vertex
fn vs_static(@location(0) pos: vec3<f32>, @location(1) uv: vec2<f32>, @location(2) color: vec4<f32>,
             @location(3) cell: vec4<f32>, @location(4) normal: vec4<f32>) -> VsOut {
    return static_vertex(pos, uv, color, cell, normal);
}

// Tekken Tag's arena: the texture coordinates its VU1 microcode makes (n.w, the batch's flags): 1 (0x16E) and 2
// (0x195) the normal in the camera's axes (x right, y down) x 0.25 (VU1 constant 0.25), + 0.5 or the stored ones;
// 8 (0x128) the reflection of the way to the eye, its height first x 3 (vf31.w), as ((x + 1) / 2, (z + 1) / 2); 4
// (0x1BC, flags 4 and 0x400) that reflection unscaled as a panorama: its bearing atan2(z, x) / 2 pi + 0.5 (EATANxz /
// EATANxy and the signs), -(y / 2 + 0.5)
fn env_uv(uv: vec2<f32>, normal: vec4<f32>, p: vec3<f32>, eye: vec3<f32>) -> vec2<f32> {
    // (16 more on a fogged strip)
    let n = vec4(normal.xyz, normal.w % 16.0);
    if (n.w < 0.5) { return uv; }
    if (n.w > 3.0 && n.w < 5.0) {
        let v = normalize(eye - p);
        let r = 2.0 * n.xyz * dot(v, n.xyz) - v;
        return vec2(atan2(r.z, r.x) / 6.2831853 + 0.5, -(r.y * 0.5 + 0.5));
    }
    if (n.w > 4.0) {
        let e = eye - p;
        let v = normalize(vec3(e.x, 3.0 * e.y, e.z));
        let r = 2.0 * n.xyz * dot(v, n.xyz) - v;
        return (r.xz + 1.0) * 0.5;
    }
    let s = 0.25 * vec2(dot(n.xyz, g.view_x.xyz), -dot(n.xyz, g.view_y.xyz));
    return s + select(vec2(0.5), uv, n.w > 1.5);
}

// Tekken Tag's arena: a fogged strip's (normal.w 16 or more; its PRIM's FGE) share of the fog colour, the GS's
// 1 - f / 255 for the VU1 block's f = 255 + slope x w (mm), clamped
fn fog_of(normal: vec4<f32>, w: f32) -> f32 {
    if (g.fog_line.w < 0.5 || normal.w < 15.5) { return 0.0; }
    return 1.0 - clamp(g.fog_line.x + g.fog_line.y * w * 1000.0, 0.0, 255.0) / 255.0;
}

// Tekken Tag's floor cell's level of detail (0x340408): the mean depth of its square's corners in front of the camera
// (none: 65.535 m); past limit k it is level k + 2 when the floor has that level (from the last down), else level 1.
fn cell_level(cell: vec4<f32>, shift: vec2<f32>) -> f32 {
    let c = (g.floor * vec4(cell.x + shift.x, 0.0, cell.y + shift.y, 1.0)).xyz;
    let h = g.floor_cell.xy;
    var sum = 0.0;
    var n = 0.0;
    for (var k = 0; k < 4; k++) {
        let s = vec2(select(-1.0, 1.0, (k & 1) == 1), select(-1.0, 1.0, (k & 2) == 2));
        let w = (g.view_proj * vec4(c.x + s.x * h.x, c.y, c.z + s.y * h.y, 1.0)).w;
        if (w > 0.0) { sum += w; n += 1.0; }
    }
    let d = select(65.535, sum / max(n, 1.0), n > 0.0);
    let last = cell.w - 1.0;
    if (last >= 5.0 && d > g.floor_levels.w) { return 5.0; }
    if (last >= 4.0 && d > g.floor_levels.z) { return 4.0; }
    if (last >= 3.0 && d > g.floor_levels.y) { return 3.0; }
    if (last >= 2.0 && d > g.floor_levels.x) { return 2.0; }
    return 1.0;
}

// A Tekken Tag floor cell's move into the window (0x341218): by whole windows, to the copy nearest the window's
// centre (its edges lie past the reach).
fn cell_shift(cell: vec4<f32>) -> vec2<f32> {
    let span = g.floor_window.xy * g.floor_window.zw;
    if (span.x <= 0.0) { return vec2(0.0); }
    return floor((g.floor_centre.xy - cell.xy) / span + 0.5) * span;
}

// Eddy's grass (0x3D43A8, 0x3E13C8): a floor vertex's move, its cell's top corners' moves (bilinearly) x its
// height in the lattice
fn grass_move(p: vec3<f32>, y: f32) -> vec3<f32> {
    let info = g.grass_info;
    if (info.w < 0.5 || y <= 0.0) { return vec3(0.0); }
    let u = clamp((p.xz - g.grass_grid.xy) / g.grass_grid.zw, vec2(0.0), info.xy - vec2(1.001));
    let c = vec2<u32>(floor(u));
    let f = u - floor(u);
    let row = u32(info.x);
    let k = c.y * row + c.x;
    let m = mix(mix(g.grass[k].xyz, g.grass[k + 1u].xyz, f.x), mix(g.grass[k + row].xyz, g.grass[k + row + 1u].xyz, f.x), f.y);
    return m * min(y / info.z, 1.0);
}

// Tekken Tag's arena scenery's z guard (Camera::scenery_cut): a corner nearer than the cut drops its triangle
fn cut_near(depth: f32, cell: vec4<f32>) -> bool {
    return g.near_cut.z > 0.0 && abs(cell.w) < 0.5 && depth < g.near_cut.z;
}

// A stage mesh's vertex, moved with the floor; a floor cell's vertex of another level than its cell's, or of a cell
// beyond the reach, goes off the picture (its triangle with it).
fn static_vertex(pos: vec3<f32>, uv: vec2<f32>, color: vec4<f32>, cell: vec4<f32>, normal: vec4<f32>) -> VsOut {
    var o: VsOut;
    let on = g.floor_cell.z > 0.5 && cell.w > 0.5;
    let shift = select(vec2(0.0), cell_shift(cell), on);
    var p = (g.floor * vec4(pos.x + shift.x, pos.y, pos.z + shift.y, 1.0)).xyz;
    if (on) { p += grass_move(p, pos.y); }
    o.clip = g.view_proj * vec4(p, 1.0);
    let away = distance(cell.xy + shift, g.floor_centre.xy) > g.floor_cell.w;
    if (on && (away || (cell.z > 0.5 && cell.z != cell_level(cell, shift)))) {
        o.clip = vec4(0.0, 0.0, 2.0, 1.0);
    }
    // Tekken Tag's arena microcode does not clip: CLIPw on each corner (GS x, y in 0..4094 about the 2048 centre, 224
    // the half height; z before 1 mm), FCAND 0x3FFFF over the triangle's three, and the ADC bit drops the triangle
    // with a corner outside
    let s = o.clip.xy / o.clip.w * vec2(g.floor_centre.z, 1.0);
    let outside = o.clip.w < 0.001 || abs(s.x) > 2047.0 / 224.0 || abs(s.y) > 2047.0 / 224.0;
    o.dropped = select(0.0, 1.0, (g.floor_cell.z > 0.5 && outside) || cut_near(o.clip.w, cell));
    o.uv = env_uv(uv, normal, p, g.eye.xyz);
    o.fog = fog_of(normal, o.clip.w);
    o.color = color;
    o.world = p;
    o.normal = vec3(0.0);
    return o;
}

// A 2D picture behind everything (an arcade stage's sky): its vertices are the screen's own (clip space), no depth.
@vertex
fn vs_backdrop(@location(0) pos: vec3<f32>, @location(1) uv: vec2<f32>, @location(2) color: vec4<f32>) -> VsOut {
    var o: VsOut;
    o.clip = vec4(pos.xy, 0.0, 1.0);
    o.uv = uv;
    o.color = color;
    o.world = vec3(0.0);
    o.normal = vec3(0.0);
    return o;
}

// The backdrop drum: rotation-only view, so it stays centred on the camera; drawn first, no depth.
@vertex
fn vs_sky(@location(0) pos: vec3<f32>, @location(1) uv: vec2<f32>, @location(2) color: vec4<f32>,
          @location(3) cell: vec4<f32>, @location(4) normal: vec4<f32>) -> VsOut {
    var o: VsOut;
    o.clip = g.sky_view_proj * vec4(pos, 1.0);
    o.dropped = select(0.0, 1.0, cut_near(o.clip.w, cell));
    o.uv = env_uv(uv, normal, pos, g.sky_eye.xyz);
    o.fog = fog_of(normal, o.clip.w);
    o.color = color;
    o.world = vec3(0.0);
    o.normal = vec3(0.0);
    return o;
}

@fragment
fn fs_unlit(i: VsOut) -> @location(0) vec4<f32> {
    if (i.dropped > 0.0) { discard; }
    let t = texel(i.uv);
    let a = cut(t.a);
    return vec4(mix(t.rgb * i.color.rgb, g.fog_colour.rgb, i.fog), a);
}

// Stage meshes: unlit, cut to the floor patch when one is set.
@fragment
fn fs_static(i: VsOut) -> @location(0) vec4<f32> {
    if (i.dropped > 0.0) { discard; }
    let c = g.floor_clip;
    if (c.x < c.y && (i.world.x < c.x || i.world.x > c.y || i.world.z < c.z || i.world.z > c.w)) { discard; }
    // walls between the camera and the fight are cut away: above the floor, inside a 2 m tube around the line from
    // the camera to either fighter's chest, and nearer the camera than that fighter by more than the margin
    if (g.cutaway.w > 0.0 && i.world.y > 0.05) {
        if (in_tube(i.world, g.cutaway.xyz, g.cutaway.w) || in_tube(i.world, g.cutaway2.xyz, g.cutaway.w)) { discard; }
        // the camera raised over the walls: everything nearer than the nearer fighter goes
        let f = g.forward.xyz;
        let near = min(dot(g.cutaway.xyz - g.eye.xyz, f), dot(g.cutaway2.xyz - g.eye.xyz, f));
        if (g.cutaway2.w > 0.5 && dot(i.world - g.eye.xyz, f) < near - g.cutaway.w) { discard; }
    }
    if (g.near_cut.y > 0.5 && i.world.y > 0.05 && i.world.z > g.near_cut.x) { discard; }
    let t = texel(i.uv);
    let a = cut(t.a);
    return vec4(mix(t.rgb * i.color.rgb * floor_light(i.world), g.fog_colour.rgb, i.fog), a);
}

fn in_tube(p: vec3<f32>, fighter: vec3<f32>, margin: f32) -> bool {
    let d = fighter + vec3(0.0, 1.0, 0.0) - g.eye.xyz;
    let v = p - g.eye.xyz;
    let len = length(d);
    let t = dot(v, d) / (len * len);
    return t < 1.0 - margin / len && length(v - t * d) < 2.0;
}

// The extra stages' floor colour (128 = 1.0, as drawn; up to 254 brightens, as the original hardware's texture modulation does)
fn fade_at(k: f32) -> f32 {
    let i = clamp(k / 32.0, 0.0, 31.0);
    let a = u32(floor(i));
    let b = min(a + 1u, 31u);
    return mix(g.fade[a / 4u][a % 4u], g.fade[b / 4u][b % 4u], i - f32(a));
}

fn floor_light(p: vec3<f32>) -> f32 {
    let mode = g.floor_light.x;
    if (mode < 0.5) { return 1.0; }
    if (mode < 1.5) {
        let sz = dot(p - g.eye.xyz, g.forward.xyz) * 1000.0;
        let k = clamp((sz - 10000.0) * 6710.0 / 65536.0, 0.0, 1023.0);
        return clamp(128.0 + fade_at(k) * g.floor_light.y / 128.0, 0.0, 254.0) / 128.0;
    }
    let a = (p.xz - g.fighters.xy) * 1000.0;
    let b = (p.xz - g.fighters.zw) * 1000.0;
    if (mode < 2.5) {
        return min(254.0, 134217728.0 / max(1.0, dot(a, a)) + 134217728.0 / max(1.0, dot(b, b))) / 128.0;
    }
    let m = (a + b) * 0.5;
    return min(192.0, 2048000000.0 / max(1.0, dot(m, m))) / 128.0;
}

// Characters: flat per-face normal from screen derivatives, one key light plus ambient
// (the engine lights per vertex with NCCS/NCCT; this is a stand-in until normals are ported).
@fragment
fn fs_lit(i: VsOut) -> @location(0) vec4<f32> {
    let t = fighter_texel(i);
    let a = cut(t.a);
    return vec4(lit(i, t.rgb), a);
}

// A character's blended primitive (Tekken Tag's): lit the same, over the picture by the texture's alpha.
@fragment
fn fs_lit_blend(i: VsOut) -> @location(0) vec4<f32> {
    let t = fighter_texel(i);
    return vec4(lit(i, t.rgb), t.a);
}

// Tekken Tag's fighters' other blendings: Cd x As (GS ALPHA 0x89) and Cs x As + Cd (0x48).
@fragment
fn fs_lit_shade(i: VsOut) -> @location(0) vec4<f32> {
    return vec4(0.0, 0.0, 0.0, fighter_texel(i).a);
}

@fragment
fn fs_lit_add(i: VsOut) -> @location(0) vec4<f32> {
    let t = fighter_texel(i);
    return vec4(lit(i, t.rgb) * t.a, 0.0);
}

// A character's texel; an environment map's repeating over its part of the texture.
fn fighter_texel(i: VsOut) -> vec4<f32> {
    if (i.env.z > 0.0) {
        return texel(i.uv + fract(i.env.xy) * i.env.zw);
    }
    return texel(i.uv);
}

fn lit(i: VsOut, t: vec3<f32>) -> vec3<f32> {
    let flat = normalize(cross(dpdx(i.world), dpdy(i.world)));
    // the hit light: a second light at the hit, in the attacker's colour, fading over 48 frames
    let to_hit = normalize(g.hit_pos.xyz - i.world + vec3(0.0, 0.0001, 0.0));
    if (g.key_color.w > 0.5) {
        // the stage's light (NCCS): ambient + key × max(0, n·L) + a fill from above at half the key; times the
        // material (128 = 1) and the texture
        var n = flat;
        if (dot(i.normal, i.normal) > 1e-8) {
            n = normalize(i.normal);
        }
        let hit = g.hit_colour.rgb * max(0.0, dot(n, to_hit));
        if (g.key_color.w > 1.5) {
            // Tekken Tag's three lights (each side's light block, 0x353918), per vertex (vs_skinned), the GS's
            // modulation saturating at 255
            return min(t * i.color.rgb, vec3(1.0));
        }
        let lum = g.ambient.rgb + g.key_color.rgb * max(0.0, dot(n, g.light_dir.xyz)) + 0.5 * g.key_color.rgb * max(0.0, n.y);
        return min(t * (g.material.rgb * lum + hit), vec3(1.0));
    }
    let l = abs(dot(flat, g.light_dir.xyz));
    let h = abs(dot(flat, to_hit));
    return t * (0.55 + 0.6 * l + g.hit_colour.rgb * h);
}

// Debug lines (hit volumes, skeleton, attack paths): world-space, vertex colour, drawn on top.
struct LineOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) color: vec4<f32>,
};

@vertex
fn vs_line(@location(0) pos: vec3<f32>, @location(1) color: vec4<f32>) -> LineOut {
    var o: LineOut;
    o.clip = g.view_proj * vec4(pos, 1.0);
    o.color = color;
    return o;
}

@fragment
fn fs_line(i: LineOut) -> @location(0) vec4<f32> {
    return i.color;
}

// Effect sprites, coloured in the game's palettes (index 0 transparent). Additive: texture × colour added;
// half: texture × colour over the picture at the colour's alpha (the dust's 50 %).
@fragment
fn fs_particle_add(i: VsOut) -> @location(0) vec4<f32> {
    let t = texel(i.uv);
    if (t.a < 0.5) { discard; }
    return vec4(t.rgb * i.color.rgb, 0.0);
}

@fragment
fn fs_particle_alpha(i: VsOut) -> @location(0) vec4<f32> {
    let t = texel(i.uv);
    if (t.a < 0.5) { discard; }
    return vec4(t.rgb * i.color.rgb, i.color.a);
}

// Tekken Tag's arenas' particles as the GS draws them (texture x vertex colour, ALPHA 0x44 by its alpha, or added)
@fragment
fn fs_particle_soft_alpha(i: VsOut) -> @location(0) vec4<f32> {
    let t = texel(i.uv);
    return vec4(t.rgb * i.color.rgb, t.a * i.color.a);
}

@fragment
fn fs_particle_soft_add(i: VsOut) -> @location(0) vec4<f32> {
    let t = texel(i.uv);
    return vec4(t.rgb * i.color.rgb, 0.0);
}

// Tekken Tag's stage strips (the GS's ALPHA 0x48 and 0x89): lights and gloss add Cs x As; painted shadows and dirt
// multiply what is behind by As (vertex alpha x texture alpha, 0x80 = 1).
@fragment
fn fs_stage_add(i: VsOut) -> @location(0) vec4<f32> {
    if (i.dropped > 0.0) { discard; }
    let t = texel(i.uv);
    return vec4(mix(t.rgb * i.color.rgb, g.fog_colour.rgb, i.fog) * t.a * i.color.a, 0.0);
}

// Tekken Tag's ordinary blend (ALPHA 0x44): the texel over the frame by its alpha times the vertex's
@fragment
fn fs_stage_blend(i: VsOut) -> @location(0) vec4<f32> {
    if (i.dropped > 0.0) { discard; }
    let t = texel(i.uv);
    return vec4(mix(t.rgb * i.color.rgb, g.fog_colour.rgb, i.fog), t.a * i.color.a);
}

@fragment
fn fs_stage_shade(i: VsOut) -> @location(0) vec4<f32> {
    if (i.dropped > 0.0) { discard; }
    let t = texel(i.uv);
    return vec4(0.0, 0.0, 0.0, t.a * i.color.a);
}
