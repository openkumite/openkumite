//! Tekken Tag Tournament's fight data on the simulation (dialect.rs): Paul against Jin on scripted pads, through the
//! stance, a walk, a jab, a string, a throw and the hits they land, run twice for the same hashes. Needs a Tekken Tag
//! `assets/` folder (ok_tekken_tag_converter's pack) in `OK_TTT_ASSETS`; skips without one.
//!
//! The bodies are stand-ins (no skeleton is posed here and no root motion read): hurt spheres stacked over the root,
//! and every bone slot at the opponent's chest, so a strike in its active frames reaches; a part starts with both in
//! the stance 800 apart.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // tests: a failure stops them

use glam::Vec3;
use kumite_sim::input::Pad;
use kumite_sim::{dialect, Animations, BodyVolumes, Fight, Fighter, GameData, Sphere, ONE};
use std::path::{Path, PathBuf};
use std::sync::Arc;

struct NoRoot;
impl Animations for NoRoot {
    fn root(&self, _: &str, _: u32, _: bool) -> Option<Vec3> {
        None
    }
}

fn body(f: &Fighter, opponent: &Fighter) -> BodyVolumes {
    let r = f.root();
    BodyVolumes {
        hurt: (0..14).map(|k| Sphere { centre: r + Vec3::Y * (150.0 + 110.0 * k as f32), radius: 250.0, zone: k }).collect(),
        push: vec![Sphere { centre: r + Vec3::Y * 900.0, radius: 300.0, zone: 0 }],
        bones: vec![opponent.root() + Vec3::Y * 1100.0; 18],
    }
}

/// (body, moveset file)
const PAUL: (i64, &str) = (0, "moveset_00.json");
const JIN: (i64, &str) = (9, "moveset_09.json");
const CYCLE: usize = 800;
/// Paul's aliases (his stance's list and the jab's): stance, walk, jab (1), its follow-up (1, 2), throw (b+2+3).
const STANCE: usize = 58;
const WALK: usize = 63;
const JAB: usize = 346;
const JAB_2: usize = 348;
const THROW: usize = 3881;

/// P1's pad at step `s` of a cycle, and the steps each part starts at.
fn pad(s: usize) -> Pad {
    let p = Pad::default();
    match s {
        40..70 => Pad { right: true, ..p },
        130 | 230 => Pad { buttons: 1, ..p },
        238 => Pad { buttons: 2, ..p },
        400 => Pad { left: true, buttons: 2 | 4, ..p },
        _ => p,
    }
}
const PARTS: [usize; 4] = [0, 120, 220, 390];

/// What a run saw, step by step: a state hash, both aliases, Jin's health, a throw link; and the hits (step, alias
/// of the attacker, damage, throw).
#[derive(Default)]
struct Run {
    hashes: Vec<u64>,
    aliases: Vec<[usize; 2]>,
    health: Vec<i32>,
    linked: Vec<bool>,
    hits: Vec<(usize, usize, i32, bool)>,
}

fn run(gd: &Arc<GameData>, dir: &Path, steps: usize) -> Run {
    let set = |m: (i64, &str)| Arc::new(dialect::moveset(gd.common.moves.len(), &dir.join("movesets").join(m.1)).unwrap());
    let mut fight = Fight::new(gd.clone(), set(PAUL), set(JIN));
    for (f, body) in fight.fighters.iter_mut().zip([PAUL.0, JIN.0]) {
        f.character = Some(body);
        f.max_health = gd.max_health_of(f.character);
        f.guard_input = false; // (standing still does not block: the hits land)
    }
    let mut out = Run::default();
    for step in 0..steps {
        let s = step % CYCLE;
        if PARTS.contains(&s) {
            fight.link = None;
            (fight.state, fight.clock) = (kumite_sim::RoundState::Fighting, kumite_sim::ROUND_FRAMES);
            for (k, f) in fight.fighters.iter_mut().enumerate() {
                let neutral = f.moveset.fixed.neutral;
                assert!(f.play_alias(neutral));
                let yaw = [1.0, -1.0][k] * std::f32::consts::FRAC_PI_2;
                (f.position, f.yaw, f.root_yaw, f.offset) = (Vec3::new([-400.0, 400.0][k], 0.0, 0.0), yaw, yaw, Vec3::ZERO);
                (f.pushback, f.air, f.freeze, f.stun, f.stun_timer, f.locked, f.thrown) = (None, None, 0, 0, 0, false, false);
                f.health = f.max_health * ONE;
            }
        }
        let [a, b] = &fight.fighters;
        let bodies = [body(a, b), body(b, a)];
        fight.step([pad(s), Pad::default()], &bodies, [&NoRoot, &NoRoot]);
        for h in &fight.hits {
            out.hits.push((step, fight.fighters[h.attacker].alias, h.damage, h.throw));
        }
        let [a, b] = &fight.fighters;
        out.aliases.push([a.alias, b.alias]);
        out.health.push(b.health / ONE);
        out.linked.push(fight.link.is_some());
        let mut h = 0xcbf2_9ce4_8422_2325u64;
        for f in &fight.fighters {
            for v in [f.position.x, f.position.z, f.yaw] {
                h = (h ^ v.to_bits() as u64).wrapping_mul(0x0100_0000_01b3);
            }
            h = (h ^ (f.health as u64) << 32 ^ f.current.index as u64 ^ (f.frame as u64) << 20).wrapping_mul(0x0100_0000_01b3);
        }
        out.hashes.push(h);
    }
    out
}

#[test]
fn paul_against_jin() {
    let Some(root) = std::env::var_os("OK_TTT_ASSETS").map(PathBuf::from) else {
        eprintln!("skipped: OK_TTT_ASSETS (a Tekken Tag assets/ folder) is not set");
        return;
    };
    let dir = root.join("gamedata");
    let gd = Arc::new(GameData::load(&dir).unwrap());
    let r = run(&gd, &dir, 4 * CYCLE);
    for (step, alias, damage, throw) in &r.hits {
        eprintln!("step {step}: Paul's alias {alias} deals {damage}{}", if *throw { " (throw)" } else { "" });
    }
    // Jin's health: the base (130) times his percent (108)
    assert_eq!(r.health[0], 140);
    for c in (0..4).map(|k| k * CYCLE) {
        let p1 = |s: std::ops::Range<usize>| r.aliases[c + s.start..c + s.end].iter().map(|a| a[0]).collect::<Vec<_>>();
        let hits = |s: std::ops::Range<usize>| r.hits.iter().filter(|h| (c + s.start..c + s.end).contains(&h.0)).map(|h| (h.1, h.2, h.3)).collect::<Vec<_>>();
        let hurt = |s: std::ops::Range<usize>| r.health[c + s.start] - r.health[c + s.end];
        // standing, then walking forward, then standing again
        assert!(p1(1..40).iter().all(|&a| a == STANCE), "stance");
        assert!(p1(40..70).contains(&WALK), "walk: {:?}", p1(40..70));
        assert_eq!(p1(100..120), vec![STANCE; 20], "back to the stance");
        // the jab hits for 5 and Jin reels (his reaction from the hit's reaction entry), then both stand again
        assert_eq!(hits(120..220), vec![(JAB, 5, false)]);
        assert_eq!(hurt(121..219), 5);
        assert!(r.aliases[c + 140][1] != STANCE, "Jin reacts");
        // 1, 2: the jab, then its follow-up
        assert_eq!(hits(220..390), vec![(JAB, 5, false), (JAB_2, 15, false)]);
        assert_eq!(hurt(221..389), 20);
        // b+2+3: the grab connects, the link pairs them, and the throw's timed damage lands
        assert!(p1(400..420).contains(&THROW), "throw: {:?}", p1(400..420));
        assert!(r.linked[c + 400..c + CYCLE].iter().any(|&l| l), "the throw links");
        let throws: Vec<_> = hits(390..CYCLE).into_iter().filter(|h| h.2).collect();
        assert!(!throws.is_empty() && hurt(391..CYCLE - 1) > 0, "the throw hurts: {throws:?}");
    }
    let again = run(&gd, &dir, 4 * CYCLE);
    assert_eq!(r.hashes, again.hashes, "the same pads, the same fight");
}
