//! The tag brawl's effect objects on the stage (tests `tag.rs` and `tekken_tag_brawl.rs`).
#![allow(clippy::unwrap_used, clippy::expect_used)] // tests: a failure stops them

use kumite_assets::Assets;
use kumite_game::tag::{Slot, TagGame, TagPhase};
use kumite_sim::input::Pad;

/// Actor 0 (`records[0]`, facing +x) plays `script` at actor 2 standing `gap` in front, actor 1 in the way halfway,
/// actor 3 off to the side, everyone else idle and not guarding: the hits (attacker, victim, limb), the frames
/// something additive is drawn within a metre of each one's root, the state hashes.
pub fn in_the_way(a: &Assets, records: [usize; 4], teams: [usize; 4], script: &str, gap: f32) -> (Vec<(usize, usize, u32)>, [usize; 4], Vec<u64>) {
    let slots = records.map(|record| Slot { record, cpu: None });
    let mut g = TagGame::load(a, slots, teams.to_vec(), kumite_game::ai_data(a).unwrap(), 2).expect("load");
    while g.phase != TagPhase::Fight {
        g.step([Pad::default(); 4]);
    }
    let east = std::f32::consts::FRAC_PI_2;
    for (i, (x, z, yaw)) in [(0.0, 0.0, east), (gap / 2.0, 0.0, -east), (gap, 0.0, -east), (gap / 2.0, 1800.0, -east)].into_iter().enumerate() {
        let f = &mut g.melee.actors[i];
        (f.position, f.yaw, f.root_yaw, f.guard_input) = (glam::Vec3::new(x, 0.0, z), yaw, yaw, false);
    }
    g.melee.target[0] = 2;
    let mut vfx = kumite_game::vfx::Vfx::new(a);
    let (mut hits, mut near, mut hashes) = (Vec::new(), [0; 4], Vec::new());
    for pad in kumite_game::parse_script(script).unwrap() {
        g.step([pad, Pad::default(), Pad::default(), Pad::default()]);
        hashes.push(g.state_hash());
        hits.extend(g.melee.hits.iter().map(|h| (h.attacker, h.victim, h.limb)));
        // (drawn as the app does)
        let models: Vec<usize> = g.bodies.iter().map(|b| b.model_record).collect();
        let actors: Vec<(&kumite_sim::Fighter, usize, usize)> = (0..4).map(|i| (&g.melee.actors[i], i, g.melee.facing(i))).collect();
        let at = |i: usize, node: usize| {
            let (pose, place) = g.pose(i);
            Some(place.transform_point3(pose.get(node)?.w_axis.truncate()))
        };
        let node = |i: usize, name: &str| g.bodies[i].character.nodes.iter().position(|n| n.name == name);
        vfx.step_actors(&g.melee.hits, &models, &actors, kumite_game::view_camera(&g.camera.view).0, &at, &node);
        vfx.objects(&g.melee.objects, &models);
        let drawn = vfx.sprites();
        for (k, n) in near.iter_mut().enumerate() {
            let root = g.melee.actors[k].root() / 1000.0;
            *n += usize::from(drawn.iter().any(|s| s.blend == kumite_game::vfx::Blend::Add && s.pos.distance(root) < 1.0));
        }
    }
    eprintln!("{records:?} {teams:?} {script}: hits {hits:?}, drawn near {near:?}");
    (hits, near, hashes)
}
