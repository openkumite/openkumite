//! Ball 2 ON 2's rules (OpenKumite's own), for both games' tests.

use kumite_assets::Assets;
use kumite_game::bout::{Bout, Phase};
use kumite_game::Game;
use kumite_sim::ball::BallEvent;
use kumite_sim::input::Pad;
use kumite_sim::{RoundState, ONE};
use std::path::Path;

/// A CPU 2 ON 2 round of `models` (the sides' first) and `team` (their partners), with `camera` (Tekken Tag: the slice's
/// Ball camera): the four keep their own health; a point costs both of the side still standing and leaves the other
/// side's alone; nothing but the ball takes health (a point, or the Attack Ball on the one it hits); one at no health
/// is nobody's target; the round is lost when both of a side are out; the same round twice is the same.
pub fn rules(a: &Assets, models: [usize; 2], team: [usize; 2], camera: Option<&Path>) {
    let run = || {
        let mut g = Game::load(a, models).unwrap();
        if let Some(c) = camera {
            g.load_camera(c).unwrap();
        }
        let data = std::sync::Arc::new(kumite_sim::data::load(&a.root.join("gamedata/ball.json")).unwrap());
        g.ball = Some(kumite_game::BallSetup { data, kind: 0, server: 1, rounds: 0 });
        g.ball_team = Some(team.map(|m| kumite_game::Actor::load(a, m).unwrap()));
        g.round_frames = u32::MAX / 2;
        (g.ko_collapse, g.fight.ko_collapse) = (false, false);
        let mut b = Bout::new(g, kumite_game::ai_data(a).unwrap(), 1, false);
        b.set_cpu(0, Some((2, 9)));
        b.set_cpu(1, Some((2, 9)));
        b.partner_cpu = Some(kumite_sim::ai::Ai::new(2, 9, 0));
        b.extra_cpu = Some(kumite_sim::ai::Ai::new(2, 9, 1));
        // (each side's two: actors 0, 3 and 1, 2)
        let order = [0, 3, 1, 2];
        let (mut log, mut points, mut out_alone) = (Vec::new(), 0, 0);
        for _ in 0..60000 {
            let f = &b.game.fight;
            let before = order.map(|i| f.actor(i).unwrap().health);
            let power = f.ball.as_ref().map_or(0, |x| x.power);
            b.step([Pad::default(); 2]);
            let f = &b.game.fight;
            let after = order.map(|i| f.actor(i).unwrap().health);
            log.push(after);
            if f.state == RoundState::Fighting && b.phase == Phase::Fight {
                let ball = f.ball.as_ref().unwrap();
                let point = ball.events.contains(&BallEvent::Impact);
                for (k, i) in order.into_iter().enumerate() {
                    if after[k] < before[k] && !point {
                        assert!(power > 0 && ball.owner == i, "actor {i} lost health without the ball: {before:?} -> {after:?}");
                    }
                }
                if point {
                    points += 1;
                    let side = usize::from(ball.pos[0] >= 0);
                    for (k, i) in order.into_iter().enumerate() {
                        let lost = if k / 2 == side && before[k] > 0 { (30 * ONE).min(before[k]) } else { 0 };
                        assert_eq!(before[k] - after[k], lost, "point on side {side}: actor {i}");
                    }
                }
                // one out (since the step before: the targets are picked at a step's start), its partner still in:
                // nobody standing faces it
                for k in 0..4 {
                    if before[k] <= 0 && after[k ^ 1] > 0 {
                        out_alone += 1;
                        let foes = if k < 2 { [1, 2] } else { [0, 3] };
                        assert!(foes.iter().all(|&o| f.actor(o).unwrap().health <= 0 || f.opponent_index(o) != order[k]), "actor {} is out and still a target", order[k]);
                    }
                }
            }
            if let RoundState::Ko { winner } = f.state {
                let lost = if winner == 0 { [2, 3] } else { [0, 1] };
                assert!(lost.iter().all(|&k| after[k] <= 0) && after[2 * winner..2 * winner + 2].iter().any(|&h| h > 0), "K.O. for {winner} at {after:?}");
                return (log, points, out_alone);
            }
        }
        panic!("no K.O.");
    };
    let (log, points, out_alone) = run();
    eprintln!("{points} points, {out_alone} steps with one out alone, {} steps", log.len());
    assert!(points >= 4 && out_alone > 0);
    assert!(run().0 == log, "the same round went another way");
}
