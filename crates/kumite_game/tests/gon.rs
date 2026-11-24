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

/// Gon's kicks strike from his feet: each kick's strike (Tekken 3's path, baked at his 45 %) ends on the drawn foot
/// of its slot (14 foot_A, 17 foot_B) on its active frames, and lands on Paul in front of him.
#[test]
fn gons_kicks_strike_from_his_feet() {
    let Some(a) = assets() else { return };
    for (script, foot) in [("f+4,n*60", "foot_A"), ("3,n*60", "foot_B"), ("d+3,n*60", "foot_B")] {
        let mut g = Game::load(&a, [GON, PAUL]).unwrap();
        g.fight.fighters[1].position.x += 3000.0;
        let mut ends = 0;
        for pad in parse_script(script).unwrap() {
            g.step([pad, Pad::default()]);
            let Some(&(_, end)) = g.fight.fighters[0].attack_segments(&g.fight.data).last() else { continue };
            let (pose, place) = g.pose(0);
            let node = g.bodies[0].character.nodes.iter().position(|n| n.name == foot).unwrap();
            let at = place.transform_point3(pose[node].w_axis.truncate()) * 1000.0;
            assert!(end.distance(at) < 40.0, "{script}: strike end {end} vs {foot} {at}");
            ends += 1;
        }
        assert!(ends > 0, "{script}: no strike");
        let near = gon(&a, script, 900.0);
        assert!(near.hits.len() == 1 && near.hits[0].1 > 0, "{script}: {near:?}");
    }
}

/// His tail (d+4: the slam down, its strike ending at slot 21, the tail's tip, past his body) reaches past his kicks:
/// it lands with 1.55 m between the two, where his hop kick (3) does not.
#[test]
fn gons_tail_reaches_past_his_kicks() {
    let Some(a) = assets() else { return };
    let tail = gon(&a, "d+4,n*60", 1550.0);
    assert!(tail.hits.len() == 1 && tail.reactions[0] != tail.stance, "{tail:?}");
    assert!(gon(&a, "3,n*60", 1550.0).hits.is_empty());
}

/// Walking into a normal-sized fighter, Gon's body stays off his: their roots no nearer than the push's floor
/// (kumite_sim::PUSH_DISTANCE) and nearer than two normal fighters' bodies keep.
#[test]
fn gon_and_paul_push_apart() {
    let Some(a) = assets() else { return };
    let gap = |m: [usize; 2]| {
        let mut g = Game::load(&a, m).unwrap();
        for pad in parse_script("f*150").unwrap() {
            g.step([pad, pad]);
        }
        let f = &g.fight.fighters;
        let d = f[0].root() - f[1].root();
        (d.x * d.x + d.z * d.z).sqrt()
    };
    let (gon, paul) = (gap([GON, PAUL]), gap([PAUL, 79]));
    eprintln!("walked in: Gon and Paul {gon}, Paul and Law {paul}");
    assert!(gon >= kumite_sim::PUSH_DISTANCE - 1.0 && gon < paul, "{gon} vs {paul}");
}
