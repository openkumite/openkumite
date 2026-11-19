//! Tekken Tag Tournament's fight data on the simulation (dialect.rs): Paul against Jin on scripted pads, through the
//! stance, a walk, a jab, a string, a throw and the hits they land, run twice for the same hashes; a launcher and its
//! juggle, a counter hit, a back-turned attack. Needs a Tekken Tag
//! `assets/` folder (ok_tekken_tag_converter's pack) in `OK_TTT_ASSETS`; skips without one.
//!
//! The bodies are stand-ins (no skeleton is posed here and no root motion read): hurt spheres stacked over the root,
//! and every bone slot at the opponent's chest, so a strike in its active frames reaches; a part starts with both in
//! the stance 800 apart.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // tests: a failure stops them

use glam::Vec3;
use kumite_sim::input::Pad;
use kumite_sim::{dialect, Animations, BodyVolumes, Fight, Fighter, GameData, HitEvent, Sphere, ONE};
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
/// d/f+2 (a launcher), and 1 back turned.
const LAUNCHER: usize = 414;
const BACK_PUNCH: usize = 374;

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

fn new_fight(gd: &Arc<GameData>, dir: &Path) -> Fight {
    let set = |m: (i64, &str)| Arc::new(dialect::moveset(gd.common.moves.len(), &dir.join("movesets").join(m.1)).unwrap());
    let mut fight = Fight::new(gd.clone(), set(PAUL), set(JIN));
    for (f, body) in fight.fighters.iter_mut().zip([PAUL.0, JIN.0]) {
        f.character = Some(body);
        f.max_health = gd.max_health_of(f.character);
        f.guard_input = false; // (standing still does not block: the hits land)
    }
    fight
}

/// Both in the stance 800 apart, facing each other, at full health.
fn reset(fight: &mut Fight) {
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

fn advance(fight: &mut Fight, pads: [Pad; 2]) {
    let [a, b] = &fight.fighters;
    let bodies = [body(a, b), body(b, a)];
    fight.step(pads, &bodies, [&NoRoot, &NoRoot]);
}

fn run(gd: &Arc<GameData>, dir: &Path, steps: usize) -> Run {
    let mut fight = new_fight(gd, dir);
    let mut out = Run::default();
    for step in 0..steps {
        let s = step % CYCLE;
        if PARTS.contains(&s) {
            reset(&mut fight);
        }
        advance(&mut fight, [pad(s), Pad::default()]);
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

/// The game data in `OK_TTT_ASSETS` (None: skipped).
fn data() -> Option<(Arc<GameData>, PathBuf)> {
    let Some(root) = std::env::var_os("OK_TTT_ASSETS").map(PathBuf::from) else {
        eprintln!("skipped: OK_TTT_ASSETS (a Tekken Tag assets/ folder) is not set");
        return None;
    };
    let dir = root.join("gamedata");
    Some((Arc::new(GameData::load(&dir).unwrap()), dir))
}

/// From the reset, `setup`, then `steps` steps of `pads`: each hit (step, event) and both aliases at each step.
fn play(gd: &Arc<GameData>, dir: &Path, setup: impl FnOnce(&mut Fight), pads: impl Fn(usize) -> [Pad; 2], steps: usize) -> (Vec<(usize, HitEvent)>, Vec<[usize; 2]>) {
    let mut fight = new_fight(gd, dir);
    reset(&mut fight);
    setup(&mut fight);
    let (mut hits, mut aliases) = (vec![], vec![]);
    for s in 0..steps {
        advance(&mut fight, pads(s));
        hits.extend(fight.hits.iter().map(|h| (s, h.clone())));
        aliases.push([fight.fighters[0].alias, fight.fighters[1].alias]);
    }
    (hits, aliases)
}

/// d/f+2 launches Jin (his reaction 3432 flies); jabs while he is in the air pick the air table's entry 0 (his alias
/// 4727) and deal 80 %, then 50 % of the jab's 5 (FIGHT_DATA.md section 9).
#[test]
fn a_launcher_then_juggle_hits() {
    let Some((gd, dir)) = data() else { return };
    let p = Pad::default();
    let pads = |s: usize| {
        let p1 = match s {
            3..=5 => Pad { down: true, right: true, buttons: if s == 5 { 2 } else { 0 }, ..p },
            30.. if s.is_multiple_of(4) => Pad { buttons: 1, ..p },
            _ => p,
        };
        [p1, p]
    };
    let (hits, aliases) = play(&gd, &dir, |_| {}, pads, 90);
    let seen: Vec<_> = hits.iter().map(|(s, h)| (aliases[*s], h.damage, h.base_damage)).collect();
    assert_eq!(seen, [([LAUNCHER, 3432], 13, 13), ([JAB, 4727], 4, 5), ([JAB, 4727], 2, 5)]);
}

/// Paul's 2 (12) on a standing Jin, and on Jin starting his 3: a counter hit, 12 x 12 / 10 and a bonus from Jin's
/// move.
#[test]
fn a_counter_hit_deals_more() {
    let Some((gd, dir)) = data() else { return };
    let p = Pad::default();
    let two = |jin: u8| {
        let pads = |s: usize| [Pad { buttons: if s == 5 { 2 } else { 0 }, ..p }, Pad { buttons: if s == 3 { jin } else { 0 }, ..p }];
        let (hits, _) = play(&gd, &dir, |_| {}, pads, 30);
        hits.iter().find(|h| h.1.attacker == 0).map(|h| (h.1.damage, h.1.counter)).unwrap()
    };
    assert_eq!(two(0), (12, false));
    let (damage, counter) = two(4);
    assert!(counter && damage > 12 * 12 / 10, "{damage}");
}

/// Back turned, 1 is Paul's back-turned punch: its entry's condition is Tekken Tag's 0x29 (back turned, right),
/// Tekken 3's 0x26 (section 7); taken as Tekken 3's 0x29 (a side quadrant) it would never pass.
#[test]
fn a_back_turned_attack() {
    let Some((gd, dir)) = data() else { return };
    let p = Pad::default();
    let pads = |s: usize| [Pad { buttons: if s == 5 { 1 } else { 0 }, ..p }, p];
    let away = |f: &mut Fight| {
        let p = &mut f.fighters[0];
        (p.yaw, p.root_yaw) = (p.yaw + 3.0, p.root_yaw + 3.0);
    };
    let (hits, aliases) = play(&gd, &dir, away, pads, 40);
    assert_eq!(aliases[6][0], BACK_PUNCH);
    assert_eq!(hits.iter().map(|h| (h.1.attacker, h.1.damage)).collect::<Vec<_>>(), [(0, 15)]);
}

#[test]
fn paul_against_jin() {
    let Some((gd, dir)) = data() else { return };
    let r = run(&gd, &dir, 4 * CYCLE);
    for (step, alias, damage, throw) in &r.hits {
        eprintln!("step {step}: Paul's alias {alias} deals {damage}{}", if *throw { " (throw)" } else { "" });
    }
    // Jin's health: the base (140, life option 3) times his percent (108)
    assert_eq!(r.health[0], 151);
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

/// Practice's QUICK ROLL dummy (0x37E0E8 / 0x37E1D8): Jin, launched, presses 3+4 only on a frame his juggle
/// reaction (posture class 9 / 10) would take it, and leaves the fall for the roll that frame.
#[test]
fn a_juggled_dummy_rolls_when_taken() {
    let Some((gd, dir)) = data() else { return };
    let mut fight = new_fight(&gd, &dir);
    reset(&mut fight);
    let p = Pad::default();
    let roll = Pad { buttons: 0xC, ..p };
    let mut pressed = None;
    for s in 0..120 {
        let p1 = if (3..=5).contains(&s) { Pad { down: true, right: true, buttons: if s == 5 { 2 } else { 0 }, ..p } } else { p };
        let jin = &fight.fighters[1];
        let class = (jin.current_move(&gd).posture.raw >> 11) & 0x1F;
        let p2 = if pressed.is_none() && matches!(class, 9 | 10) && jin.accepts(&gd, roll, fight.facing_right(1)) { roll } else { p };
        let before = jin.alias;
        advance(&mut fight, [p1, p2]);
        if p2 == roll {
            pressed = Some((s, before, fight.fighters[1].queued()));
        }
    }
    let (s, before, queued) = pressed.expect("the roll was never taken");
    eprintln!("roll at step {s}: alias {before}, queued {queued:?}");
    assert!(queued.is_some());
}
