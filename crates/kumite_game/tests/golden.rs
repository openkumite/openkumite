//! The golden guard: fixed matches (CPU bouts, scripted pads, a Force stage, Ball, the tag brawl) whose state hashes,
//! chained step by step, must stay what golden.json holds: a change of the simulation that changes a fight of this
//! game fails here, with the scenario and the steps it went apart in.
//!
//! The goldens are those of one conversion (golden.json's key: the game data's fingerprint, manifest.json's); with
//! other game data, or none, the test skips with a note. The scenarios of an optional pack (`t2 ...`) run only with
//! the pack there. After a change that is meant to change the fights, write them again and commit the file:
//!
//! ```text
//! OK_GOLDEN_WRITE=1 OPENKUMITE_ASSETS=<the game data> cargo test --release -p kumite_game --test golden
//! ```
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // tests: a failure stops them

use kumite_assets::Assets;
use kumite_game::bout::{self, Bout, Phase};
use kumite_game::force::{ForceData, ForceEvent, ForceStage};
use kumite_game::tag::{Slot, TagGame};
use kumite_game::{parse_script, Actor, BallSetup, Game};
use kumite_sim::ai::Ai;
use kumite_sim::input::Pad;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

/// The chain is written down every so many steps (a second): a mismatch is told to these steps.
const MARK: usize = 60;

/// A scenario's hash chain: every step's state hash folded in, its low 32 bits noted every `MARK` steps and at the
/// end.
#[derive(Default)]
struct Chain {
    h: u64,
    steps: usize,
    marks: String,
}

impl Chain {
    fn eat(&mut self, hash: u64) {
        self.h = (self.h.rotate_left(5) ^ hash).wrapping_mul(0x0100_0000_01b3);
        self.steps += 1;
        if self.steps.is_multiple_of(MARK) {
            self.mark();
        }
    }
    fn mark(&mut self) {
        self.marks += &format!("{:08x}", self.h as u32);
    }
}

/// The steps two chains' marks first differ in (None: the same).
fn apart(got: &str, want: &str) -> Option<(usize, usize)> {
    let (g, w) = (got.as_bytes().chunks(8), want.as_bytes().chunks(8));
    let k = g.zip(w).position(|(a, b)| a != b).or((got.len() != want.len()).then_some(got.len().min(want.len()) / 8))?;
    Some((k * MARK + 1, (k + 1) * MARK))
}

/// Character `c`'s model in costume `k` (the first sixteen characters' records lie 8 apart).
fn model(c: usize, k: usize) -> usize {
    71 + 8 * c.min(15) + 4 * k.min(1)
}

/// A match of `models`: each side a CPU (difficulty, level) or its pad script, over and over.
fn fight(a: &Assets, models: [usize; 2], cpu: [Option<(usize, usize)>; 2], scripts: [&str; 2], ball: bool, steps: usize, c: &mut Chain) {
    let mut game = Game::load(a, models).unwrap();
    // (the copying character's draw of the round: the match's seed)
    game.copy_seed = 7;
    if ball {
        // as the app sets Ball up: the clock off, no K.O. collapse
        game.ball = Some(BallSetup { data: Arc::new(kumite_sim::data::load(&a.root.join("gamedata/ball.json")).unwrap()), kind: 0, server: 0, rounds: 0 });
        game.round_frames = u32::MAX / 2;
        (game.ko_collapse, game.fight.ko_collapse) = (false, false);
    }
    let mut b = Bout::new(game, kumite_game::ai_data(a).unwrap(), 2, true);
    b.set_cpu(0, cpu[0]);
    b.set_cpu(1, cpu[1]);
    let scripts = scripts.map(|s| parse_script(s).unwrap());
    for step in 0..steps {
        b.step(scripts.each_ref().map(|s| s.get(step % s.len().max(1)).copied().unwrap_or_default()));
        c.eat(b.game.state_hash());
    }
}

/// Force's stage `index` as the app plays it alone (tests/force_ai.rs: the soldiers at the default difficulty, the
/// bosses when they are done), the player an AI seat.
fn force(a: &Assets, index: usize, p: usize, steps: usize, c: &mut Chain) {
    let data: Arc<ForceData> = Arc::new(kumite_sim::data::load(&a.root.join("gamedata/force.json")).unwrap());
    let soldier = |k: usize| 259 + 4 * data.soldier_costumes[index][k].min(3);
    let soldiers = [0, 1].map(|k| Actor::load(a, soldier(k)).unwrap());
    let mut game = Game::load(a, [model(p, 0), soldier(0)]).unwrap();
    game.round_frames = kumite_game::force::CLOCK;
    (game.ko_collapse, game.fight.ko_collapse) = (false, false);
    let mut bout = Bout::new(game, kumite_game::ai_data(a).unwrap(), 1, false);
    bout.force = true;
    let (d, l) = (1, data.ai_level(index));
    bout.set_cpu(0, Some((2, 9)));
    bout.set_cpu(1, Some((d, l)));
    bout.extra_cpu = Some(Ai::new(d, l, 1));
    let mut st = ForceStage::new(data.clone(), index, soldiers, 4 * p, p, 0, 0, 0, 0);
    st.setup(&mut bout.game);
    for _ in 0..steps {
        bout.step([Pad::default(); 2]);
        let won = matches!(bout.phase, Phase::RoundOver { winner: Some(0) } | Phase::MatchOver { winner: Some(0) });
        st.step(&mut bout.game, bout.phase == Phase::Fight, won);
        for e in std::mem::take(&mut st.events) {
            match e {
                ForceEvent::Swapped => bout.swap_cpus(),
                ForceEvent::LoadBoss(ch, cos) => {
                    st.set_boss(&mut bout.game, &Actor::load(a, model(ch, cos)).unwrap(), (ch, cos));
                    bout.set_cpu(1, Some((d, l)));
                }
                _ => {}
            }
        }
        // (the stage's own state as online play's checksum takes it: app/src/netplay.rs)
        c.eat(bout.game.state_hash() ^ (st.cam_x as u64).rotate_left(17) ^ (st.score as u64).rotate_left(41) ^ (st.state as u64) << 56);
    }
}

/// The tag brawl of four CPUs in `teams`.
fn tag(a: &Assets, records: [usize; 4], teams: [usize; 4], steps: usize, c: &mut Chain) {
    let slots = records.map(|record| Slot { record, cpu: Some((2, 9)) });
    let mut g = TagGame::load(a, slots, teams.to_vec(), kumite_game::ai_data(a).unwrap(), 2).unwrap();
    for _ in 0..steps {
        g.step([Pad::default(); 4]);
        c.eat(g.state_hash());
    }
}

// (movement, the four attacks, low and command moves, both throws, sidesteps either way, a jump, dashes)
const P1: &str = "f*30,n*3,1,n*8,2,n*10,1+3,n*90,b*12,n*2,u,n*30,d,n*30,f,n*2,f*12,n*4,d/f+2,n*30,3,n*12,4,n*20,d*15,d+4,n*20,u*3,n*50,b,n*2,b,n*30,f*20,2+4,n*90,u/f*3,n*50,4,n*30";
const P2: &str = "n*45,b*10,d/f+1,n*20,f*25,1,n,2,n*20,d,n*25,2+4,n*80,d+3,n*20,u,n*25,f*20,3,n*30,d/b*20,4,n*40";

/// Every scenario's marks, by its name.
fn scenarios(a: &Assets) -> BTreeMap<String, String> {
    let index: kumite_sim::data::MovesetIndex = kumite_sim::data::load(&a.root.join("gamedata/movesets/index.json")).unwrap();
    let t2 = index.models.iter().any(|m| m.model_record == 900);
    let cpus = [Some((1, 6)), Some((2, 9))];
    type Run<'a> = Box<dyn Fn(&mut Chain) + 'a>;
    // (models: 71 Paul, 79 Law, 95 King, 111 Nina, 127 Xiaoyu, 135 Eddy, 159 Kuma, 191 Mokujin, 199 True Ogre, 207 Gun
    // Jack; the Tekken 2 pack's 900 Kazuya, 906 Jack-2, 938 Jun)
    let mut runs: Vec<(&str, Run)> = vec![
        ("cpu paul law", Box::new(|c| fight(a, [71, 79], cpus, ["", ""], false, 3000, c))),
        ("cpu mokujin kuma", Box::new(|c| fight(a, [191, 159], cpus, ["", ""], false, 3000, c))),
        ("cpu true ogre gun jack", Box::new(|c| fight(a, [199, 207], [Some((2, 9)); 2], ["", ""], false, 3000, c))),
        ("cpu king nina", Box::new(|c| fight(a, [95, 111], [Some((0, 2)), Some((1, 4))], ["", ""], false, 3000, c))),
        ("cpu mokujin mokujin", Box::new(|c| fight(a, [191, 195], cpus, ["", ""], false, 3000, c))),
        ("pads xiaoyu eddy", Box::new(|c| fight(a, [127, 135], [None, None], [P1, P2], false, 3000, c))),
        ("pads paul against cpu king", Box::new(|c| fight(a, [71, 95], [None, Some((1, 5))], [P1, ""], false, 3000, c))),
        ("pads kuma against cpu law", Box::new(|c| fight(a, [79, 159], [Some((1, 5)), None], ["", P1], false, 3000, c))),
        ("force stage 1", Box::new(|c| force(a, 0, 0, 6000, c))),
        ("ball paul law", Box::new(|c| fight(a, [71, 79], cpus, ["", ""], true, 3000, c))),
        ("tag 2 on 2", Box::new(|c| tag(a, [71, 79, 103, 87], [0, 0, 1, 1], 3000, c))),
        ("tag four way", Box::new(|c| tag(a, [159, 111, 191, 143], [0, 1, 2, 3], 3000, c))),
    ];
    if t2 {
        runs.push(("t2 cpu kazuya jun", Box::new(|c| fight(a, [900, 938], cpus, ["", ""], false, 3000, c))));
        runs.push(("t2 pads jack-2 against cpu paul", Box::new(|c| fight(a, [906, 71], [None, Some((1, 5))], [P1, ""], false, 3000, c))));
    } else {
        eprintln!("the Tekken 2 pack is not there: its scenarios skipped");
    }
    let mut out = BTreeMap::new();
    for (name, run) in runs {
        // (the camera counters run on from bout to bout: every scenario from zero)
        bout::set_counters([0; 3]);
        let (mut c, t) = (Chain::default(), std::time::Instant::now());
        run(&mut c);
        c.mark();
        eprintln!("{name}: {} steps, {} ms", c.steps, t.elapsed().as_millis());
        out.insert(name.to_string(), c.marks);
    }
    out
}

#[test]
fn the_fights_are_the_goldens() {
    let root = std::env::var_os("OPENKUMITE_ASSETS").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets"));
    let Ok(a) = Assets::new(&root).map_err(|e| eprintln!("skipping: {e:#}")) else { return };
    let mut goldens: BTreeMap<String, BTreeMap<String, String>> = serde_json::from_str(include_str!("golden.json")).unwrap();
    let fingerprint = a.fingerprint();
    if std::env::var_os("OK_GOLDEN_WRITE").is_some() {
        assert!(!fingerprint.is_empty(), "the game data has no fingerprint");
        goldens.insert(fingerprint, scenarios(&a));
        std::fs::write(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/golden.json"), serde_json::to_string_pretty(&goldens).unwrap() + "\n").unwrap();
        return;
    }
    let Some(golden) = goldens.get(&fingerprint) else {
        eprintln!("skipping: golden.json has no goldens of this game data (fingerprint {fingerprint:?}; it has {:?})", goldens.keys().collect::<Vec<_>>());
        return;
    };
    let mut wrong = Vec::new();
    for (name, got) in scenarios(&a) {
        match golden.get(&name).map(|want| apart(&got, want)) {
            None => wrong.push(format!("{name}: no golden (OK_GOLDEN_WRITE=1 writes it)")),
            Some(Some((from, to))) => wrong.push(format!("{name}: went apart in steps {from}..={to}")),
            Some(None) => {}
        }
    }
    assert!(wrong.is_empty(), "the simulation is not the goldens':\n{}", wrong.join("\n"));
}

#[test]
fn chains_apart_name_their_steps() {
    assert_eq!(apart("0000000011111111", "0000000011111111"), None);
    assert_eq!(apart("0000000011111111", "0000000022222222"), Some((61, 120)));
    assert_eq!(apart("00000000", "0000000022222222"), Some((61, 120)));
}
