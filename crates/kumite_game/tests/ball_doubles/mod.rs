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
/// is nobody's target, nor framed by the camera (its standing partner is, in its place); everyone standing is in the
/// picture sideways; the round is lost when both of a side are out; the same round twice is the same.
pub fn rules(a: &Assets, models: [usize; 2], team: [usize; 2], camera: Option<&Path>) {
    let run = |server: usize| {
        let mut g = Game::load(a, models).unwrap();
        if let Some(c) = camera {
            g.load_camera(c).unwrap();
        }
        let data = std::sync::Arc::new(kumite_sim::data::load(&a.root.join("gamedata/ball.json")).unwrap());
        g.ball = Some(kumite_game::BallSetup { data, kind: 0, server, rounds: 0 });
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
        let (mut log, mut points, mut out_alone, mut framed, mut off, mut seen) = (Vec::new(), 0, 0, 0, 0, 0);
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
                // everyone standing in the picture sideways (the root, 900 up: 256 of the 512 pixels each way)
                let (eye, forward, fovy) = kumite_game::view_camera(&b.game.camera.view);
                let right = forward.cross(glam::Vec3::Y).normalize_or_zero();
                for i in order.into_iter().filter(|&i| f.actor(i).unwrap().health > 0) {
                    let d = (f.actor(i).unwrap().root() + glam::Vec3::Y * 900.0) / kumite_game::UNITS - eye;
                    let depth = d.dot(forward);
                    seen += 1;
                    off += usize::from(depth <= 0.0 || (d.dot(right) / depth).abs() > (fovy / 2.0).tan() * 256.0 / 240.0);
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
            // (the samples the next step's camera takes)
            for (i, s) in b.game.camera_samples().iter().enumerate() {
                let (me, mate) = (f.actor(i).unwrap(), f.actor(3 - i).unwrap());
                let out = me.health <= 0 && mate.health > 0;
                framed += usize::from(out);
                let at = if out { mate.root() } else { me.root() };
                assert_eq!((s.x, s.z), (at.x.round() as i64, -(at.z.round() as i64)), "side {i}'s framed player");
            }
            if let RoundState::Ko { winner } = f.state {
                let lost = if winner == 0 { [2, 3] } else { [0, 1] };
                assert!(lost.iter().all(|&k| after[k] <= 0) && after[2 * winner..2 * winner + 2].iter().any(|&h| h > 0), "K.O. for {winner} at {after:?}");
                return (log, points, out_alone, framed, off, seen);
            }
        }
        panic!("no K.O.");
    };
    // (both servers: a round where a side's first falls before its partner)
    let (mut out_alone, mut framed, mut off, mut seen) = (0, 0, 0, 0);
    for server in [1, 0] {
        let (log, points, o, f, x, n) = run(server);
        eprintln!("server {server}: {points} points, {o} steps with one out alone ({f} framed in a first's place), {} steps, {x} of {n} off the picture", log.len());
        assert!(points >= 4);
        (out_alone, framed, off, seen) = (out_alone + o, framed + f, off + x, seen + n);
        assert!(run(server).0 == log, "the same round went another way");
    }
    assert!(out_alone > 0 && framed > 0);
    // (a moment off while the eye catches up with a dash)
    assert!(off * 20 < seen, "{off} of {seen} standing off the picture");
}
