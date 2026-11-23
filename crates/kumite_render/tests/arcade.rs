//! An arcade stage's model is drawn against the floor's depth: what of it lies under the floor is hidden (the
//! helipad's tower below its deck), as the arcade shows it. On a GPU; skipped without one.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // tests: a failure stops them

use glam::Vec3;
use kumite_render::{Camera, Renderer, StaticVertex};

#[test]
fn the_floor_hides_the_model_under_it() {
    let Ok(mut r) = Renderer::headless(64, 48).map_err(|e| eprintln!("skipping: {e:#}")) else { return };
    let white = r.texture(1, 1, &[255; 4]);
    let quad = |c: [[f32; 3]; 4], color: [f32; 4]| -> Vec<StaticVertex> { c.map(|pos| StaticVertex { pos, uv: [0.0; 2], color, cell: [0.0; 4], normal: [0.0; 4] }).to_vec() };
    let idx = [0, 1, 2, 0, 2, 3];
    // a green floor at y 0, and a red wall of the model 10 m away, all of it under the floor
    let floor = r.static_mesh(&quad([[-50.0, 0.0, -50.0], [50.0, 0.0, -50.0], [50.0, 0.0, 50.0], [-50.0, 0.0, 50.0]], [0.0, 1.0, 0.0, 1.0]), &idx, white.clone());
    let wall = r.sorted_mesh(&quad([[-20.0, -30.0, -10.0], [20.0, -30.0, -10.0], [20.0, -0.5, -10.0], [-20.0, -0.5, -10.0]], [1.0, 0.0, 0.0, 1.0]), &idx, white);
    // looking down through the floor at the wall
    let mut cam = Camera::new(Vec3::new(0.0, 2.0, 5.0), Vec3::new(0.0, -1.0, -10.0), 0.8);
    (cam.sky_cull, cam.sky_depth) = (false, true);
    r.render_with(&cam, &[&wall], &[&floor], &[], None, &[], None).unwrap();
    let img = r.capture().unwrap();
    let p = &img.rgba[((img.height / 2 * img.width + img.width / 2) * 4) as usize..][..4];
    assert!(p[1] > p[0], "the wall over the floor: {p:?}");
}
