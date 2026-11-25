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

/// Through the round's call to the fight, P1 in its intro alias 6 and Paul in 4 (× and □ held at the call: the
/// starts these tests were set on).
fn to_fight(b: &mut Bout) {
    b.step(["3", "1"].map(|k| parse_script(k).unwrap()[0]));
    while b.phase != Phase::Fight {
        b.step([Pad::default(); 2]);
    }
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
    to_fight(&mut b);
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
/// in Tekken 3): it is hit and plays reactions; then it jabs Paul, who backs off: Paul is hit (Doctor B. walks in
/// between his jabs: his jab from a stand falls short of Paul backing off).
#[test]
fn hit_and_hit_back() {
    let Some(a) = assets() else { return };
    let walk_jabs = format!("f*300,{}n", "f*10,1,n*5,".repeat(30));
    for (p1, me, paul) in [(GON, "f*300,JABS", "LOWS,b"), (DOCTOR_B, walk_jabs.as_str(), "JABS,b")] {
        let r = run(&a, p1, [me, paul], 800);
        assert!(r.missing.is_empty(), "model {p1}: no motion {:?}", &r.missing[..r.missing.len().min(5)]);
        let hurt = r.health.iter().position(|h| h[0] < r.health[0][0]).expect("Paul's attack lands");
        assert!(r.aliases[hurt..hurt + 10].iter().any(|a| a[0] != r.aliases[hurt - 1][0]), "model {p1}: a reaction");
        assert!(r.health.last().unwrap()[1] < r.health[0][1], "model {p1}: Paul is hit");
        assert_eq!(r.hashes, run(&a, p1, [me, paul], 800).hashes, "the same pads, the same match");
    }
}

/// Gon's fire (1+2) and gas (b+4) in Tekken Tag: their objects strike Paul (a fire's hit, a gas's), who loses health
/// and reacts; drawn from Tekken 3's pictures the install put beside Tekken Tag's (effects.json `t3`).
#[test]
fn gons_fire_and_gas() {
    let Some(a) = assets() else { return };
    for (moves, byte) in [("1+2,n*120", kumite_sim::objects::FIRE), ("f*60,b+4,n*120", kumite_sim::objects::GAS)] {
        let mut b = Bout::new(Game::load(&a, [GON, PAUL]).unwrap(), kumite_game::ai_data(&a).unwrap(), 2, false);
        to_fight(&mut b);
        b.game.fight.fighters[1].guard_input = false;
        let mut vfx = kumite_game::vfx::Vfx::new(&a);
        let sheet = vfx.data.as_ref().map_or(0, |d| d.objects_sheet);
        assert!(sheet > 0, "Tekken 3's pictures");
        let (mut hit, mut objects, mut drawn) = (None, 0, 0);
        for pad in parse_script(moves).unwrap() {
            b.step([pad, Pad::default()]);
            vfx.step(&b.game);
            objects = objects.max(b.game.fight.objects.list.len());
            hit = hit.or(b.game.fight.hits.iter().find(|h| h.victim == 1).map(|h| (h.limb, h.damage)));
            drawn += usize::from(vfx.sprites().iter().any(|s| s.sheet == sheet));
        }
        assert!(objects > 0 && hit.is_some_and(|(limb, d)| limb == byte && d > 0), "{moves}: {objects} objects, hit {hit:?}");
        // drawn from Tekken 3's sheet: the gas's puffs a second, the fire and Paul's burning longer
        assert!(drawn >= 60, "{moves}: drawn on {drawn} frames");
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
const LEI: usize = 372;
const HWOARANG: usize = 420;

/// A tag battle, `team` against Paul and Jin, the tag button on fight frame `at` (`lying`: □ held for the round
/// start, Doctor B.'s falling one): each frame P1's alias, its team's active member, its model, the state hash, the
/// fighters' distance (game units); and how many frames a fighter had no motion on.
fn tag_out(a: &Assets, team: [usize; 2], lying: bool, at: usize) -> (Vec<(usize, usize, usize, u64, f32)>, usize) {
    let mut g = Game::load(a, [team[0], PAUL]).unwrap();
    g.start_tag([kumite_game::Actor::load(a, team[1]).unwrap(), kumite_game::Actor::load(a, JIN).unwrap()]);
    let mut b = Bout::new(g, kumite_game::ai_data(a).unwrap(), 2, false);
    // (P1 lying holds 1 (its intro alias 4, then the fall), else × (alias 6); Paul □ at the call (4))
    let start = parse_script(if lying { "1" } else { "3" }).unwrap()[0];
    b.step([start, parse_script("1").unwrap()[0]]);
    while b.phase != Phase::Fight {
        b.step([if lying { start } else { Pad::default() }, Pad::default()]);
    }
    let tag = parse_script("5").unwrap()[0];
    let mut missing = 0;
    let r = (0..at + 200)
        .map(|k| {
            b.step([if k == at { tag } else { Pad::default() }, Pad::default()]);
            missing += (0..2).filter(|&i| b.game.clip(i).is_none()).count();
            let t = &b.game.fight.teams.as_ref().unwrap()[0];
            let f = &b.game.fight.fighters;
            (f[0].alias, t.active, b.game.bodies[0].model_record, b.game.state_hash(), f[0].position.distance(f[1].position))
        })
        .collect();
    (r, missing)
}

/// The made-up tag moves: the tag button starts the run off where Tekken Tag tags (Gon's stance: alias 5394, his
/// backdash; Doctor B.'s crouched guard too; lying face down, Tekken Tag's roll off 5438), the partner comes in on
/// the entrance (5397: Tekken Tag's run in, from off the screen to the opponent), then its stance (Doctor B.: his
/// guard, 80). Every frame on a motion; the same twice.
#[test]
fn tag_out_and_in() {
    let Some(a) = assets() else { return };
    for (team, lying, at, run) in [([GON, DOCTOR_B], false, 1, 5394), ([PAUL, GON], false, 1, 5394), ([DOCTOR_B, PAUL], false, 1, 5394), ([DOCTOR_B, GON], true, 60, 5438)] {
        let (r, missing) = tag_out(&a, team, lying, at);
        let alias: Vec<usize> = r.iter().map(|x| x.0).collect();
        assert!(alias[at..at + 3].contains(&run), "model {}: the tag button starts the run off: {alias:?}", team[0]);
        let swap = r.iter().position(|x| x.1 == 1).unwrap_or_else(|| panic!("the swap: {alias:?}"));
        assert_eq!((r[swap].0, r[swap].2), (5397, team[1]), "the partner's entrance");
        assert!(r[swap..].iter().all(|x| x.1 == 1) && alias[swap..].iter().any(|a| [58, 80].contains(a)), "one swap, then the stance: {alias:?}");
        let ended = swap + alias[swap..].iter().position(|&a| a != 5397).unwrap();
        assert!(r[ended].4 < 2500.0, "model {}: in at {} from the opponent", team[1], r[ended].4);
        assert_eq!(missing, 0);
        assert_eq!(r.iter().map(|x| x.3).collect::<Vec<_>>(), tag_out(&a, team, lying, at).0.iter().map(|x| x.3).collect::<Vec<_>>());
    }
}

/// CPU against CPU: the Tekken 3 fighter against a Tekken Tag one; per side the attacks started and the clean hits
/// (in the fight: a K.O.'s replay shows its hits again), the frames without a motion, the state hashes.
fn cpu_fight(a: &Assets, m: [usize; 2]) -> ([u32; 2], [u32; 2], usize, Vec<u64>) {
    let mut b = Bout::new(Game::load(a, m).unwrap(), kumite_game::ai_data(a).unwrap(), 2, false);
    b.set_cpu(0, Some((1, 5)));
    b.set_cpu(1, Some((1, 5)));
    let (mut starts, mut hits, mut missing, mut hashes, mut last) = ([0; 2], [0; 2], 0, Vec::new(), [0; 2]);
    for _ in 0..2400 {
        let fight = b.phase == Phase::Fight;
        b.step([Pad::default(); 2]);
        hashes.push(b.game.state_hash());
        missing += (0..2).filter(|&i| b.game.clip(i).is_none()).count();
        for (k, f) in b.game.fight.fighters.iter().enumerate() {
            if f.alias != last[k] && f.current_move(&b.game.fight.data).active[0] != 0 {
                starts[k] += 1;
            }
            last[k] = f.alias;
        }
        for h in b.game.fight.hits.iter().filter(|h| fight && !h.blocked && h.attacker < 2) {
            hits[h.attacker] += 1;
        }
    }
    (starts, hits, missing, hashes)
}

/// The CPU plays Gon and Doctor B. against Paul and Lei: it attacks and lands about as many hits as in Tekken 3's own
/// game (6 to 12 a fight there; Gon landed about 3 with his root at 100 %) and is hit, every frame on a motion; the
/// same twice.
#[test]
fn cpu_plays_them() {
    let Some(a) = assets() else { return };
    for p1 in [GON, DOCTOR_B] {
        let mut landed = [0; 2];
        for p2 in [PAUL, LEI] {
            let (starts, hits, missing, hashes) = cpu_fight(&a, [p1, p2]);
            eprintln!("model {p1} against {p2}: attacks {starts:?}, hits {hits:?}");
            assert!(starts[0] > 10 && missing == 0, "model {p1} against {p2}: attacks {starts:?}, {missing} frames without a motion");
            assert_eq!(hashes, cpu_fight(&a, [p1, p2]).3);
            landed = [landed[0] + hits[0], landed[1] + hits[1]];
        }
        assert!(landed[0] >= 10 && landed[1] > 0, "model {p1}: hits {landed:?}");
    }
}

/// Gon's motions' root runs at his model's size (45 %, the roster's `models`), as in Tekken 3: his 2 from 1000 units
/// hits an idle Paul. At 100 % his root ran 2.2 times as far as his strike, his body pushed Paul out of its reach and
/// the CPU landed a third of his Tekken 3 hits.
#[test]
fn gons_root_runs_at_his_size() {
    let Some(a) = assets() else { return };
    let mut g = Game::load(&a, [GON, PAUL]).unwrap();
    for _ in 0..300 {
        g.step([Pad::default(); 2]);
    }
    g.fight.fighters[1].guard_input = false;
    g.fight.fighters[1].position.x = g.fight.fighters[0].position.x + 1000.0;
    let pads = parse_script("2").unwrap();
    let hit = (0..40).any(|k| {
        g.step([pads.get(k).copied().unwrap_or_default(), Pad::default()]);
        g.fight.hits.iter().any(|h| h.attacker == 0)
    });
    assert!(hit, "Gon's 2 from 1000 units");
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
    to_fight(&mut b);
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

/// Doctor B.'s timed sound scripts (Tekken 3's, renumbered): his backflip (motion_040, script 154) plays his voice
/// on frame 2 (his own bank) and Tekken 3's take-off and landing on frames 24 and 39 (his scripts' bank, past his
/// voices').
#[test]
fn doctor_bs_scripts_sound() {
    let Some(a) = assets() else { return };
    let s = SoundData::load(&a).unwrap();
    let sound: serde_json::Value = a.gamedata("sound.json").unwrap();
    let own = sound["voice_banks"]["characters"].as_array().unwrap().iter().find(|v| v["model_record"] == DOCTOR_B).unwrap()["bank"].as_u64().unwrap();
    let mut b = Bout::new(Game::load(&a, [DOCTOR_B, PAUL]).unwrap(), kumite_game::ai_data(&a).unwrap(), 2, false);
    to_fight(&mut b);
    let mut st = SoundState::default();
    heard(&a, &s, &b, &mut st);
    let f = &b.game.fight.fighters[0];
    let flip = f.moveset.aliases.as_ref().unwrap().iter().position(|r| r.index.is_some_and(|i| f.moveset.moves[i].anim == "motion_040")).expect("the backflip");
    b.game.fight.fighters[0].play_alias(flip);
    let mut files = Vec::new();
    for _ in 0..40 {
        b.step([Pad::default(); 2]);
        files.extend(heard(&a, &s, &b, &mut st));
    }
    let bank = |f: &String| f.split('/').next().and_then(|k| k.parse::<u64>().ok()).unwrap_or(0);
    assert!(files.iter().any(|f| bank(f) == own) && files.iter().filter(|f| bank(f) > own).count() >= 2, "{files:?}");
}

/// Doctor B.'s jumps that Tekken 3's type 9 entry 0 mutes play no take-off under Tekken Tag either (0x376578): the
/// same move without the entry plays one more sound.
#[test]
fn doctor_bs_type_9_mutes_his_jumps() {
    let Some(a) = assets() else { return };
    let s = SoundData::load(&a).unwrap();
    let play = |strip: bool| {
        let mut b = Bout::new(Game::load(&a, [DOCTOR_B, PAUL]).unwrap(), kumite_game::ai_data(&a).unwrap(), 2, false);
        to_fight(&mut b);
        let mut st = SoundState::default();
        heard(&a, &s, &b, &mut st);
        let f = &mut b.game.fight.fighters[0];
        let muted = |m: &kumite_sim::data::Move| m.sounds.iter().flatten().any(|e| e.kind == 9 && e.index == 0);
        let aliases = f.moveset.aliases.clone().unwrap();
        let alias = aliases.iter().position(|r| r.index.is_some_and(|i| f.moveset.moves[i].bytes_18.jump_start > 1 && muted(&f.moveset.moves[i]))).expect("a muted jump");
        let k = aliases[alias].index.unwrap();
        if strip {
            std::sync::Arc::make_mut(&mut f.moveset).moves[k].sounds.iter_mut().for_each(|l| l.retain(|e| e.kind != 9));
        }
        f.play_alias(alias);
        let mut files = Vec::new();
        for _ in 0..f.moveset.moves[k].bytes_18.jump_start + 4 {
            b.step([Pad::default(); 2]);
            files.extend(heard(&a, &s, &b, &mut st));
        }
        files
    };
    let (muted, plain) = (play(false), play(true));
    assert_eq!(plain.len(), muted.len() + 1, "{muted:?} {plain:?}");
}

/// Xiaoyu's first model.
const XIAOYU: usize = 432;

/// What a Ball match of two CPUs saw: the frames the ball was charged, the most charge, the fighters' lowest health,
/// the Ball sounds heard (files of the slice's Ball bank), the state hashes.
fn ball_match(a: &Assets, models: [usize; 2]) -> (u32, i32, [i32; 2], Vec<String>, Vec<u64>) {
    let mut g = Game::load(a, models).unwrap();
    // as the app sets Ball up (the slice's ball.json in the view): the clock off, no K.O. collapse
    g.ball = Some(kumite_game::BallSetup { data: std::sync::Arc::new(a.gamedata("ball.json").unwrap()), kind: 0, server: 0, rounds: 0 });
    g.round_frames = u32::MAX / 2;
    (g.ko_collapse, g.fight.ko_collapse) = (false, false);
    let mut b = Bout::new(g, kumite_game::ai_data(a).unwrap(), 2, false);
    b.set_cpu(0, Some((1, 5)));
    b.set_cpu(1, Some((1, 5)));
    let s = SoundData::load(a).unwrap();
    let sound: serde_json::Value = a.gamedata("sound.json").unwrap();
    let serve = a.gamedata::<serde_json::Value>("ball.json").unwrap()["constants"]["sounds"]["serve"].as_u64().unwrap();
    let group = sound["groups"]["entries"].as_array().unwrap().iter().find(|g| g["first_id"].as_u64() == Some(serve)).expect("the Ball group");
    let own = format!("{:03}/", group["bank"].as_u64().unwrap());
    let (mut st, mut charged, mut most, mut low, mut sounds, mut hashes) = (SoundState::default(), 0, 0, [i32::MAX; 2], Vec::new(), Vec::new());
    for _ in 0..3000 {
        b.step([Pad::default(); 2]);
        hashes.push(b.game.state_hash());
        let ball = b.game.fight.ball.as_ref().unwrap();
        charged += u32::from(ball.power > 0);
        most = most.max(ball.charge[0].max(ball.charge[1]));
        for (k, f) in b.game.fight.fighters.iter().enumerate() {
            low[k] = low[k].min(f.health / kumite_sim::ONE);
        }
        sounds.extend(heard(a, &s, &b, &mut st).into_iter().filter(|f| f.starts_with(&own)));
    }
    (charged, most, low, sounds, hashes)
}

/// Tekken Ball under Tekken Tag (the slice's ball.json, Tekken Tag's numbers): Paul against Gon, Jin against Xiaoyu,
/// two CPUs. The ball is hit and charged (its power from the hitting move's damage, as Tekken 3's), health is lost
/// (points, Attack Ball hits), the Ball sounds play; the same twice.
#[test]
fn ball_under_tekken_tag() {
    let Some(a) = assets() else { return };
    for models in [[PAUL, GON], [JIN, XIAOYU]] {
        let (charged, most, low, sounds, hashes) = ball_match(&a, models);
        eprintln!("{models:?}: charged {charged} frames, most {most}, lowest health {low:?}, {} Ball sounds", sounds.len());
        assert!(charged > 0 && most > 0, "{models:?}: the ball is charged");
        assert!(low.iter().any(|&h| h < 100), "{models:?}: health lost {low:?}");
        assert!(!sounds.is_empty(), "{models:?}: no Ball sound");
        assert_eq!(hashes, ball_match(&a, models).4, "{models:?}: the same twice");
    }
}

/// A model record of character `c` in Tekken Tag's roster.json: its costume `k`'s (else its first), or selection
/// `c` itself when `k` is None (the slice's soldiers: Crow's).
fn record_of(a: &Assets, c: usize, k: Option<usize>) -> usize {
    let roster: serde_json::Value = a.gamedata("roster.json").unwrap();
    let list = roster["selections"]["entries"].as_array().unwrap();
    let e = match k {
        None => list.iter().find(|e| e["selection"] == c),
        Some(k) => list.iter().filter(|e| e["character"] == c && e["model_record"].is_u64()).nth(k).or_else(|| list.iter().find(|e| e["character"] == c && e["model_record"].is_u64())),
    };
    e.unwrap()["model_record"].as_u64().unwrap() as usize
}

/// What a Force stage saw: the camera's way, the kills, the soldiers' hits taken and their reactions (another alias
/// within 10 frames of a hit), the player's hits taken, the boss met and its health lost, frames without a motion,
/// the state hashes.
#[derive(Debug, Default)]
struct ForceRun {
    progress: i32,
    kills: u32,
    soldier_hits: u32,
    reactions: u32,
    player_hits: u32,
    boss: Option<(usize, usize)>,
    boss_lost: i32,
    missing: usize,
    hashes: Vec<u64>,
}

/// Stage 1 of Tekken Force under Tekken Tag (the slice's force.json: Crow's soldiers, the bosses in Tekken Tag's
/// numbers) as the app plays it, the AI playing `player` (hard); `boss`: straight to the boss. Up to `frames` frames.
fn force_stage(a: &Assets, player: usize, boss: bool, frames: usize) -> ForceRun {
    use kumite_game::force::{ForceData, ForceEvent, ForceStage};
    use kumite_game::Actor;
    let data: std::sync::Arc<ForceData> = std::sync::Arc::new(a.gamedata("force.json").unwrap());
    let soldier = |k: usize| record_of(a, data.soldier_selection_base + data.soldier_costumes[0][k].min(3), None);
    let soldiers = [0, 1].map(|k| Actor::load(a, soldier(k)).unwrap());
    let mut game = Game::load(a, [player, soldier(0)]).unwrap();
    game.round_frames = kumite_game::force::CLOCK;
    (game.ko_collapse, game.fight.ko_collapse) = (false, false);
    let mut b = Bout::new(game, kumite_game::ai_data(a).unwrap(), 1, false);
    b.force = true;
    let (d, l) = (1, data.ai_level(0));
    b.set_cpu(1, Some((d, l)));
    b.extra_cpu = Some(kumite_sim::ai::Ai::new(d, l, 1));
    // (the app's selection in Tekken Tag: body x 8 + costume)
    let body = b.game.fight.fighters[0].character.unwrap_or(0).max(0) as usize;
    let mut st = ForceStage::new(data.clone(), 0, soldiers, body * 8, body, 0, 0, 0, 0);
    st.setup(&mut b.game);
    while b.phase != Phase::Fight {
        b.step([Pad::default(); 2]);
    }
    b.set_cpu(0, Some((2, 9)));
    b.cpu[0] = Some(kumite_sim::ai::Ai::new(2, 9, 0));
    if boss {
        st.skip_to_boss(&mut b.game);
    }
    let mut r = ForceRun::default();
    let mut hit_at: Option<(usize, usize, usize)> = None;
    for k in 0..frames {
        // (the soldiers' aliases before the step: a hit's reaction may start in it)
        let before: Vec<usize> = (0..3).map(|i| b.game.fight.actor(i).map_or(0, |x| x.alias)).collect();
        b.step([Pad::default(); 2]);
        let fighting = b.phase == Phase::Fight;
        let won = matches!(b.phase, Phase::RoundOver { winner: Some(0) } | Phase::MatchOver { winner: Some(0) });
        st.step(&mut b.game, fighting, won);
        for e in std::mem::take(&mut st.events) {
            match e {
                ForceEvent::Swapped => b.swap_cpus(),
                ForceEvent::LoadBoss(ch, cos) => {
                    st.set_boss(&mut b.game, &Actor::load(a, record_of(a, ch, Some(cos))).unwrap(), (ch, cos));
                    b.set_cpu(1, Some((d, l)));
                    r.boss = Some((ch, cos));
                }
                _ => {}
            }
        }
        let f = &b.game.fight;
        for h in f.hits.iter().filter(|h| !h.blocked) {
            if h.attacker == 0 {
                r.soldier_hits += 1;
                hit_at = Some((k, h.victim, before.get(h.victim).copied().unwrap_or(0)));
            } else if h.victim == 0 {
                r.player_hits += 1;
            }
        }
        if let Some((at, v, alias)) = hit_at {
            if f.actor(v).is_some_and(|x| x.alias != alias) {
                r.reactions += 1;
                hit_at = None;
            } else if k > at + 10 {
                hit_at = None;
            }
        }
        if r.boss.is_some() {
            r.boss_lost = f.fighters[1].max_health / kumite_sim::ONE - f.fighters[1].health / kumite_sim::ONE;
        }
        r.missing += (0..2).filter(|&i| !f.fighters[i].inactive && b.game.clip(i).is_none()).count();
        r.hashes.push(b.game.state_hash());
        if st.tally_done || f.fighters[0].health <= 0 {
            break;
        }
    }
    (r.progress, r.kills) = (st.cam_x, st.kills_total);
    r
}

/// Tekken Force under Tekken Tag (the slice's force.json): Paul and Jin (the AI playing them) fight Crow's soldiers
/// through stage 1's first area (the camera at its second arena, 11776): the soldiers are hit and react, they hit the
/// player, every frame on a motion (Lei's and Hwoarang's hits make them play moves their aliases take from the
/// player's list, on the player's motions); then the stage's boss (a Tekken Tag fighter, by Tekken 3's route) comes in and is
/// fought; the same twice.
#[test]
fn force_under_tekken_tag() {
    let Some(a) = assets() else { return };
    if a.gamedata::<serde_json::Value>("force.json").is_err() {
        eprintln!("skipped: no Tekken Force in the slice");
        return;
    }
    // its sounds (the countdown voices, the chicken's) are ids of a group of their own, its final music a BGM of Tekken 3's
    let f: serde_json::Value = a.gamedata("force.json").unwrap();
    let s = SoundData::load(&a).unwrap();
    for code in f["constants"]["countdown"]["codes"].as_array().unwrap().iter().chain([&f["constants"]["chicken"]["sfx"], &f["constants"]["tally"]["sfx"]]) {
        assert!(s.system(code.as_u64().unwrap() as u16).is_some_and(|x| kumite_fs::exists(&x.file)), "Force sound {code}");
    }
    assert!(s.music(f["constants"]["final"]["bgm"].as_u64().unwrap() as usize).is_some_and(|m| kumite_fs::exists(&m.file)));
    for player in [PAUL, JIN, LEI, HWOARANG] {
        let r = force_stage(&a, player, false, 60 * 60 * 3);
        eprintln!(
            "model {player}: progress {}, kills {}, hits {} / {} reactions, hit {} times, {} frames without a motion",
            r.progress, r.kills, r.soldier_hits, r.reactions, r.player_hits, r.missing
        );
        // (Lei and Hwoarang are here for their moves on the soldiers, not to clear the area)
        assert!(r.kills >= 5 && (r.progress >= 11776 || ![PAUL, JIN].contains(&player)), "model {player}: the first area: {r:?}");
        assert!(r.soldier_hits > 0 && r.reactions > 0 && r.player_hits > 0 && r.missing == 0, "model {player}: {:?}", (r.soldier_hits, r.reactions, r.player_hits, r.missing));
        let boss = force_stage(&a, player, true, 60 * 40);
        eprintln!("model {player}: boss {:?}, its health lost {}", boss.boss, boss.boss_lost);
        assert!(boss.boss.is_some() && boss.boss_lost > 0 && boss.missing == 0, "model {player}: the boss fight {:?}", (boss.boss, boss.boss_lost, boss.missing));
        assert_eq!(boss.hashes, force_stage(&a, player, true, 60 * 40).hashes, "model {player}: the same twice");
    }
}

/// Every reaction a Force soldier can receive under Tekken Tag has a motion: each fighter (each body's first model,
/// Gon's and Doctor B.'s too) against each soldier, every victim alias of the reactions, the air reactions and the throws' victims, and
/// the moves they go on to, played on the soldier: its own motion or, for a move its aliases take from the player's
/// list, the player's (`Game::clip_owner`).
#[test]
fn every_reaction_has_a_motion_on_a_soldier() {
    let Some(a) = assets() else { return };
    let Ok(data) = a.gamedata::<kumite_game::force::ForceData>("force.json") else {
        eprintln!("skipped: no Tekken Force in the slice");
        return;
    };
    let index: serde_json::Value = a.gamedata("movesets/index.json").unwrap();
    let mut players: Vec<(u64, usize)> = Vec::new();
    for m in index["models"].as_array().unwrap() {
        let body = m["body"].as_u64().unwrap();
        if !players.iter().any(|p| p.0 == body) {
            players.push((body, m["model_record"].as_u64().unwrap() as usize));
        }
    }
    let soldiers: Vec<usize> = (0..2).map(|k| record_of(&a, data.soldier_selection_base + data.soldier_costumes[0][k].min(3), None)).collect();
    let (mut missing, mut played) = (Vec::new(), 0);
    for &(_, player) in &players {
        for &soldier in &soldiers {
            let mut g = Game::load(&a, [player, soldier]).unwrap();
            g.step([Pad::default(); 2]);
            let gd = g.fight.data.clone();
            let r = &gd.reactions;
            let mut todo: Vec<usize> =
                r.entries.iter().flat_map(|e| e.victims.iter().copied()).chain(r.air_reactions.entries.iter().map(|e| e.alias)).chain(r.throw_victims.entries.iter().copied()).collect();
            let mut seen = std::collections::BTreeSet::new();
            while let Some(alias) = todo.pop() {
                if alias == 0 || !seen.insert(alias) || !g.fight.fighters[1].play_alias(alias) {
                    continue;
                }
                played += 1;
                if g.clip(1).is_none() {
                    missing.push((player, soldier, alias, g.fight.fighters[1].animation(&gd).0.to_string()));
                }
                todo.push(g.fight.fighters[1].current_move(&gd).next.alias);
            }
        }
    }
    eprintln!("{} players, {} soldiers: {played} moves played", players.len(), soldiers.len());
    assert!(played > 1000 && missing.is_empty(), "{} without a motion: {:?}", missing.len(), &missing[..missing.len().min(40)]);
}

/// Walking into Paul (both forward), Gon's body stays off his in Tekken Tag's push (its spheres, 0x377338): their
/// roots no nearer than Paul and Law's less a third.
#[test]
fn gon_and_paul_push_apart() {
    let Some(a) = assets() else { return };
    let gap = |m: [usize; 2]| {
        let mut b = Bout::new(Game::load(&a, m).unwrap(), kumite_game::ai_data(&a).unwrap(), 2, false);
        to_fight(&mut b);
        let mut least = f32::MAX;
        for pad in parse_script("f*150").unwrap() {
            b.step([pad, pad]);
            let f = &b.game.fight.fighters;
            let d = f[0].root() - f[1].root();
            least = least.min((d.x * d.x + d.z * d.z).sqrt());
        }
        least
    };
    let (gon, paul) = (gap([GON, PAUL]), gap([PAUL, 348]));
    eprintln!("walked in: Gon and Paul {gon}, Paul and Law {paul}");
    assert!(gon > paul * 2.0 / 3.0, "{gon} vs {paul}");
}

/// Gon walks, backs off and crouches in Tekken Tag: drawn as in Tekken 3, his head stays above his pelvis through the
/// pose ease too (its turns are in the PS1's axes: drawn as Tekken Tag's, they turned him over).
#[test]
fn eased_poses_stay_upright() {
    let Some(a) = assets() else { return };
    let mut b = Bout::new(Game::load(&a, [GON, PAUL]).unwrap(), kumite_game::ai_data(&a).unwrap(), 2, false);
    to_fight(&mut b);
    let (mut eased, mut over) = (0, Vec::new());
    for (k, pad) in parse_script(&"f*20,n*10,b*20,n*10,d*20,n*10,".repeat(3)).unwrap().into_iter().enumerate() {
        b.step([pad, Pad::default()]);
        eased += usize::from(b.game.fight.fighters[0].eased().is_some());
        let (pose, place) = b.game.pose_animated_for_tests(0);
        let body = &b.game.bodies[0];
        let y = |n: usize| place.transform_point3(pose[n].w_axis.truncate()).y;
        if y(body.chest) < y(body.pelvis) {
            over.push(k);
        }
    }
    assert!(eased > 0, "an ease");
    assert!(over.is_empty(), "turned over on frames {over:?}");
}
