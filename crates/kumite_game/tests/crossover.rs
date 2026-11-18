//! Tekken 3's Gon and Doctor B. in Tekken Tag Tournament (its extras, installed here from both games' data:
//! kumite_packs::install_ttt): against Paul with their real skeletons posed, each gets hit and reacts, hits back and
//! makes Paul react, every frame on a motion its body has; a throw links; two runs give the same state hashes. Needs
//! a Tekken 3 game root (OPENKUMITE_ASSETS) and a Tekken Tag one (OK_TTT_ASSETS); skips without either.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // tests: a failure stops them

use kumite_assets::Assets;
use kumite_game::bout::{Bout, Phase};
use kumite_game::sound::{SoundData, SoundState};
use kumite_game::{parse_script, Game};
use kumite_sim::input::Pad;
use ok_converter_common::io::StdFs;
use std::path::PathBuf;
use std::sync::OnceLock;

/// Paul's first model; Gon's and Doctor B.'s (Tekken 3's model records).
const PAUL: usize = 345;
const GON: usize = 239;
const DOCTOR_B: usize = 247;

/// Tekken Tag's game root with the extras installed over it (once for the tests).
fn assets() -> Option<Assets> {
    let (Some(t3), Some(ttt)) = (std::env::var_os("OPENKUMITE_ASSETS"), std::env::var_os("OK_TTT_ASSETS")) else {
        eprintln!("skipped: OPENKUMITE_ASSETS or OK_TTT_ASSETS unset");
        return None;
    };
    static EXTRAS: OnceLock<PathBuf> = OnceLock::new();
    let extras = EXTRAS.get_or_init(|| {
        let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("ttt_crossover_extras");
        kumite_packs::install_ttt(&StdFs(ttt.clone().into()), &StdFs(t3.into()), &StdFs(dir.clone()), &|l| eprintln!("{l}")).unwrap();
        dir
    });
    kumite_fs::set_layers(std::path::Path::new(&ttt), vec![extras.clone()]);
    Some(Assets::new(std::path::Path::new(&ttt)).unwrap())
}

/// What a run saw: per frame the state hash, both aliases, both healths; whether a throw linked; frames on which a
/// fighter had no motion for its move.
#[derive(Default)]
struct Run {
    hashes: Vec<u64>,
    aliases: Vec<[usize; 2]>,
    health: Vec<[i32; 2]>,
    linked: bool,
    missing: Vec<(usize, usize, String)>,
}

/// `p1` against Paul from the fight's start: the scripts (P1's, P2's; JABS: 30 jabs 16 frames apart, LOWS: 10 low
/// kicks, THROWS: 10 throws; then the last pad held).
fn run(a: &Assets, p1: usize, scripts: [&str; 2], frames: usize) -> Run {
    let scripts = scripts.map(|s| s.replace("JABS", &"1,n*15,".repeat(30)).replace("LOWS", &"d+3,n*30,".repeat(10)).replace("THROWS", &"1+3,n*20,".repeat(10)));
    let mut b = Bout::new(Game::load(a, [p1, PAUL]).unwrap(), kumite_game::ai_data(a).unwrap(), 2, false);
    while b.phase != Phase::Fight {
        b.step([Pad::default(); 2]);
    }
    let pads = scripts.map(|s| parse_script(&s).unwrap());
    let mut r = Run::default();
    for k in 0..frames {
        b.step([0, 1].map(|i| pads[i][k.min(pads[i].len() - 1)]));
        r.hashes.push(b.game.state_hash());
        let f = &b.game.fight.fighters;
        r.aliases.push([f[0].alias, f[1].alias]);
        r.health.push([f[0].health / kumite_sim::ONE, f[1].health / kumite_sim::ONE]);
        r.linked |= b.game.fight.link.is_some();
        for (i, f) in f.iter().enumerate().filter(|&(i, _)| b.game.clip(i).is_none()) {
            r.missing.push((k, i, f.animation(&b.game.fight.data).0.to_string()));
        }
    }
    r
}

/// The Tekken 3 fighter walks in (no guard while walking) and Paul attacks (low kicks on Gon: highs whiff over him, as
/// in Tekken 3): it is hit and plays reactions; then Paul walks in on its jabs: Paul is hit.
#[test]
fn hit_and_hit_back() {
    let Some(a) = assets() else { return };
    for (p1, paul) in [(GON, "LOWS,b"), (DOCTOR_B, "JABS,b")] {
        let r = run(&a, p1, ["f*300,JABS", paul], 800);
        assert!(r.missing.is_empty(), "model {p1}: no motion {:?}", &r.missing[..r.missing.len().min(5)]);
        let hurt = r.health.iter().position(|h| h[0] < r.health[0][0]).expect("Paul's attack lands");
        assert!(r.aliases[hurt..hurt + 10].iter().any(|a| a[0] != r.aliases[hurt - 1][0]), "model {p1}: a reaction");
        assert!(r.health.last().unwrap()[1] < r.health[0][1], "model {p1}: Paul is hit");
        assert_eq!(r.hashes, run(&a, p1, ["f*300,JABS", paul], 800).hashes, "the same pads, the same match");
    }
}

/// Paul's throw (1+3) on Doctor B. walking in, and Gon's on Paul: both link, every frame on a motion its body has.
/// (Gon, Tekken 3's unthrowable fighter, is too small for Paul's.)
#[test]
fn throws() {
    let Some(a) = assets() else { return };
    let r = run(&a, DOCTOR_B, ["f*300", "THROWS"], 300);
    assert!(r.linked && r.missing.is_empty(), "Paul throws Doctor B.: {:?}", &r.missing[..r.missing.len().min(5)]);
    let r = run(&a, GON, ["f*40,1+3,n*10", "b*200"], 300);
    assert!(r.linked && r.missing.is_empty(), "Gon throws Paul: {:?}", &r.missing[..r.missing.len().min(5)]);
}

/// Jin's first model: Paul's partner.
const JIN: usize = 465;

/// A tag battle, `team` against Paul and Jin, the tag button on frame 1: each frame P1's alias, its team's active
/// member, its model, the state hash; and how many frames a fighter had no motion on.
fn tag_out(a: &Assets, team: [usize; 2]) -> (Vec<(usize, usize, usize, u64)>, usize) {
    let mut g = Game::load(a, [team[0], PAUL]).unwrap();
    g.start_tag([kumite_game::Actor::load(a, team[1]).unwrap(), kumite_game::Actor::load(a, JIN).unwrap()]);
    let mut b = Bout::new(g, kumite_game::ai_data(a).unwrap(), 2, false);
    while b.phase != Phase::Fight {
        b.step([Pad::default(); 2]);
    }
    let tag = parse_script("5").unwrap()[0];
    let mut missing = 0;
    let r = (0..200)
        .map(|k| {
            b.step([if k == 1 { tag } else { Pad::default() }, Pad::default()]);
            missing += (0..2).filter(|&i| b.game.clip(i).is_none()).count();
            let t = &b.game.fight.teams.as_ref().unwrap()[0];
            (b.game.fight.fighters[0].alias, t.active, b.game.bodies[0].model_record, b.game.state_hash())
        })
        .collect();
    (r, missing)
}

/// The made-up tag moves: Gon's tag button starts his run off (alias 5394: his backdash) and Doctor B. comes in on
/// his entrance (5397: his round intro); Paul tagging out brings Gon in on his. Every frame on a motion; the same
/// twice.
#[test]
fn tag_out_and_in() {
    let Some(a) = assets() else { return };
    for team in [[GON, DOCTOR_B], [PAUL, GON]] {
        let (r, missing) = tag_out(&a, team);
        let alias: Vec<usize> = r.iter().map(|x| x.0).collect();
        assert!(alias.contains(&5394), "the tag button starts the run off: {alias:?}");
        let swap = r.iter().position(|x| x.1 == 1).unwrap_or_else(|| panic!("the swap: {alias:?}"));
        assert_eq!((r[swap].0, r[swap].2), (5397, team[1]), "the partner's entrance");
        assert!(r[swap..].iter().all(|x| x.1 == 1) && alias[swap..].contains(&58), "one swap, then the stance: {alias:?}");
        assert_eq!(missing, 0);
        assert_eq!(r.iter().map(|x| x.3).collect::<Vec<_>>(), tag_out(&a, team).0.iter().map(|x| x.3).collect::<Vec<_>>());
    }
}

/// CPU against CPU: the Tekken 3 fighter against Paul; per side the attacks started and the clean hits, the frames
/// without a motion, the state hashes.
fn cpu_fight(a: &Assets, p1: usize) -> ([u32; 2], [u32; 2], usize, Vec<u64>) {
    let mut b = Bout::new(Game::load(a, [p1, PAUL]).unwrap(), kumite_game::ai_data(a).unwrap(), 2, false);
    b.set_cpu(0, Some((1, 5)));
    b.set_cpu(1, Some((1, 5)));
    let (mut starts, mut hits, mut missing, mut hashes, mut last) = ([0; 2], [0; 2], 0, Vec::new(), [0; 2]);
    for _ in 0..2400 {
        b.step([Pad::default(); 2]);
        hashes.push(b.game.state_hash());
        missing += (0..2).filter(|&i| b.game.clip(i).is_none()).count();
        for (k, f) in b.game.fight.fighters.iter().enumerate() {
            if f.alias != last[k] && f.current_move(&b.game.fight.data).active[0] != 0 {
                starts[k] += 1;
            }
            last[k] = f.alias;
        }
        for h in b.game.fight.hits.iter().filter(|h| !h.blocked && h.attacker < 2) {
            hits[h.attacker] += 1;
        }
    }
    (starts, hits, missing, hashes)
}

/// The CPU plays Gon and Doctor B.: it attacks and lands hits, every frame on a motion; the same twice.
#[test]
fn cpu_plays_them() {
    let Some(a) = assets() else { return };
    for p1 in [GON, DOCTOR_B] {
        let (starts, hits, missing, hashes) = cpu_fight(&a, p1);
        eprintln!("model {p1}: attacks {starts:?}, hits {hits:?}");
        assert!(starts[0] > 10 && hits[0] > 0 && missing == 0, "model {p1}: attacks {starts:?}, hits {hits:?}, {missing} frames without a motion");
        assert_eq!(hashes, cpu_fight(&a, p1).3);
    }
}

/// What the sound tables make a step play: the files under audio/sfx.
fn heard(a: &Assets, s: &SoundData, b: &Bout, st: &mut SoundState) -> Vec<String> {
    s.step(b, st, ["", ""], [0, 0]).iter().map(|x| x.file.strip_prefix(a.root.join("audio/sfx")).unwrap().to_string_lossy().into_owned()).collect()
}

/// Their sounds (sound.json, its voice groups added): Gon's hit on Paul and Paul's on Gon play the impacts (bank 1);
/// a voiced move of Gon's plays a sample of his own bank, and so does his K.O.
#[test]
fn hits_and_voices_sound() {
    let Some(a) = assets() else { return };
    let s = SoundData::load(&a).unwrap();
    let sound: serde_json::Value = a.gamedata("sound.json").unwrap();
    let bank = sound["voice_banks"]["characters"].as_array().unwrap().iter().find(|v| v["model_record"] == GON).unwrap()["bank"].as_u64().unwrap();
    let own = format!("{bank:03}/");
    let mut b = Bout::new(Game::load(&a, [GON, PAUL]).unwrap(), kumite_game::ai_data(&a).unwrap(), 2, false);
    while b.phase != Phase::Fight {
        b.step([Pad::default(); 2]);
    }
    let mut st = SoundState::default();
    // each side's hit until one lands: (Gon jabs, Paul walks in), then (Gon walks in, Paul kicks low)
    let hit = |b: &mut Bout, st: &mut SoundState, scripts: [&str; 2], attacker: usize| {
        let pads = scripts.map(|s| parse_script(s).unwrap());
        (0..600).find_map(|k| {
            b.step([0, 1].map(|i| pads[i][k % pads[i].len()]));
            let heard = heard(&a, &s, b, st);
            b.game.fight.hits.iter().any(|h| h.attacker == attacker && !h.blocked).then_some(heard)
        })
    };
    let gon_hits = hit(&mut b, &mut st, ["1,n*15", "b"], 0).expect("Gon's jab lands");
    assert!(gon_hits.iter().any(|f| f.starts_with("001/")), "Gon's hit: {gon_hits:?}");
    let paul_hits = hit(&mut b, &mut st, ["f", "d+3,n*30"], 1).expect("Paul's low kick lands");
    assert!(paul_hits.iter().any(|f| f.starts_with("001/")), "Paul's hit: {paul_hits:?}");
    // a move of Gon's with a voice (type 2) on its first frame
    for _ in 0..90 {
        b.step([Pad::default(); 2]);
        heard(&a, &s, &b, &mut st);
    }
    let gon = &b.game.fight.fighters[0];
    let voiced = gon.moveset.aliases.as_ref().unwrap().iter().position(|r| r.index.is_some_and(|i| gon.moveset.moves[i].sounds.iter().flatten().any(|e| e.kind == 2))).expect("a voiced move");
    b.game.fight.fighters[0].play_alias(voiced);
    let voice = heard(&a, &s, &b, &mut st);
    assert!(voice.iter().any(|f| f.starts_with(&own)), "Gon's voice: {voice:?}");
    // his K.O.: a voice of his own bank
    b.game.fight.fighters[0].health = kumite_sim::ONE;
    let ko = hit(&mut b, &mut st, ["f", "d+3,n*30"], 1).expect("the K.O.");
    assert!(ko.iter().any(|f| f.starts_with(&own)), "Gon's K.O.: {ko:?}");
}
