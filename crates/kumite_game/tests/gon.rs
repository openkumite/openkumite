//! Gon's fire (1+2) and gas (b+4): their objects start, travel and strike as the game's (kumite_sim::objects); the
//! struck fighter takes the move's damage and reaction. Needs Tekken 3's game data (OPENKUMITE_ASSETS); skips without.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // tests: a failure stops them

use kumite_assets::Assets;
use kumite_game::{parse_script, Game};
use kumite_sim::input::Pad;
use kumite_sim::objects::{Kind, FIRE, GAS};
use std::path::PathBuf;

const GON: usize = 239;
const PAUL: usize = 71;

fn assets() -> Option<Assets> {
    let root = std::env::var_os("OPENKUMITE_ASSETS").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets"));
    Assets::new(&root).map_err(|e| eprintln!("skipping: {e:#}")).ok()
}

/// What Gon's move did: the hits on P2 (frame, damage, limb), the most objects seen, the farthest a fire went from
/// Gon (units), P2's moves after the hit and before it.
#[derive(Debug, Default)]
pub struct Seen {
    hits: Vec<(usize, i32, u32)>,
    objects: usize,
    reach: f32,
    reactions: Vec<String>,
    stance: String,
}

/// Gon (P1) plays `script` against an idle Paul standing `gap` units apart (the fight starts them 2000 apart).
fn gon(a: &Assets, script: &str, gap: f32) -> Seen {
    let mut g = Game::load(a, [GON, PAUL]).unwrap();
    g.fight.fighters[1].guard_input = false;
    g.fight.fighters[1].position.x = g.fight.fighters[0].position.x + gap;
    let mut s = Seen { stance: g.fight.fighters[1].current_move(&g.fight.data).anim.clone(), ..Seen::default() };
    for (k, pad) in parse_script(script).unwrap().into_iter().enumerate() {
        g.step([pad, Pad::default()]);
        let f = &g.fight;
        s.objects = s.objects.max(f.objects.list.len());
        for o in f.objects.list.iter().filter(|o| o.kind == Kind::Fire) {
            s.reach = s.reach.max((o.pos - f.fighters[0].root()).length());
        }
        s.hits.extend(f.hits.iter().filter(|h| h.victim == 1).map(|h| (k, h.damage, h.limb)));
        if !s.hits.is_empty() && s.reactions.len() < 3 {
            s.reactions.push(f.fighters[1].current_move(&f.data).anim.clone());
        }
    }
    eprintln!("{script} at {gap}: {s:?}");
    s
}

/// 1+2: the fire leaves Gon's mouth on frame 46 and strikes Paul 1.5 m away (unblockable); 4 m away it ends short
/// of him, after more than 2 m.
#[test]
fn gons_fire_strikes_at_range_and_ends() {
    let Some(a) = assets() else { return };
    let near = gon(&a, "1+2,n*100", 1500.0);
    assert!(near.objects >= 1 && near.hits.len() == 1 && near.hits[0].2 == FIRE && near.hits[0].1 > 0, "{near:?}");
    assert!(near.hits[0].0 >= 45 && near.reactions[0] != near.stance, "from the active frame on, a reaction");
    let far = gon(&a, "1+2,n*100", 4000.0);
    assert!(far.hits.is_empty() && far.reach > 2000.0 && far.reach < 4000.0, "{far:?}");
}

/// b+4: Gon turns his back, the gas's puffs come out at Paul and strike him close by; 2.5 m off, nothing.
#[test]
fn gons_gas_strikes_close_by() {
    let Some(a) = assets() else { return };
    let near = gon(&a, "b+4,n*100", 700.0);
    assert!(near.objects >= 8 && near.hits.len() == 1 && near.hits[0].2 == GAS && near.hits[0].1 > 0, "{near:?}");
    assert!(near.reactions[0] != near.stance, "a reaction");
    let far = gon(&a, "b+4,n*100", 2500.0);
    assert!(far.hits.is_empty() && far.objects >= 8, "{far:?}");
}
