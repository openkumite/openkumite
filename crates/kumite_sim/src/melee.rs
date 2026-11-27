//! Tag brawl (OpenKumite's own mode, not in the original): four on the stage at once, in two teams of two or each
//! for themselves. Every actor fights its target, the nearest standing actor of another team, with the versus
//! rules: the pair logic of [`Fight`] (throws, body push, hits) runs on each pair by moving the two actors into
//! the fight's two slots. The round ends when one team (or nobody) is left standing.

use crate::detmath::DetMath;
use std::sync::Arc;

use glam::Vec3;

use crate::data::Moveset;
use crate::input::Pad;
use crate::{sight_of, Animations, Bank, BodyVolumes, Current, Fight, Fighter, GameData, HitEvent, ONE, ROUND_FRAMES, TRANSITION_LINK};

/// How the round stands (winners are teams).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MeleeState {
    Fighting,
    Ko {
        winner: usize,
    },
    /// Everyone went down together.
    DoubleKo,
    TimeUp {
        winner: Option<usize>,
    },
}

/// A retarget waits until another opponent is this much closer (game units).
pub const RETARGET_MARGIN: f32 = 300.0;
/// The tag brawl's picks count a foe straight behind as this much further (game units; to the side half of it):
/// a foe closing in behind is taken on when clearly nearer, not each time he edges past the one in front, which
/// would leave the actor turning round again and again.
pub const BEHIND_COST: f32 = 600.0;
/// Nobody strays further than this from the group's centre (game units): the camera keeps all four in view
/// without leaving the stage's backdrop (two fighters alone rarely stand 4 m apart).
pub const LEASH: f32 = 2000.0;

/// A round of the tag brawl.
#[derive(Clone)]
pub struct Melee {
    /// The pair slots and the shared rules (frame counter, screen direction, guard damage, K.O. collapse).
    pub fight: Fight,
    pub actors: Vec<Fighter>,
    /// Each actor's team (0, 1; each for themselves 0..3).
    pub team: Vec<usize>,
    /// Each actor's target (an actor of the other team).
    pub target: Vec<usize>,
    /// Each actor's target is locked on with TARGET (OpenKumite's own): kept while it stands in play.
    pub locked: Vec<bool>,
    /// Each actor's TARGET held last step (a new press cycles).
    target_held: Vec<bool>,
    /// The throws / holds in progress: the pair and the moves that linked them.
    pub links: Vec<(usize, usize, [Current; 2])>,
    pub clock: u32,
    pub state: MeleeState,
    /// Hits of the last step (attacker / victim are actor indices).
    pub hits: Vec<HitEvent>,
    /// The distance nobody strays beyond from the group's centre (None: no leash).
    pub leash: Option<f32>,
    /// How much further a foe straight behind counts in the picks of a target (game units; Force's co-op: 0,
    /// the nearest).
    pub behind_cost: f32,
    /// Tekken Tag's pair rules on each pair, as its pair fight's (the tag brawl under Tekken Tag): the distance
    /// limit, the facing from the roots before the moves, the pushback in the facing pass, the push and hits in the
    /// pose of the frame.
    pub tag_pairs: bool,
    /// Each pair's kept places for Tekken Tag's distance limit ([`Fight::kept`]): the pair, the fight's state.
    pub kept: Vec<(usize, usize, KeptPlaces)>,
    /// The effect objects that strike (owners: actor indices); each pair's hit pass takes its two's.
    pub objects: crate::objects::Objects,
}

/// [`Fight::kept`]'s state.
pub type KeptPlaces = ([[i32; 2]; 2], [u32; 2]);

impl Melee {
    /// Actors 0, 1 are team 0 (on the left), 2, 3 team 1.
    pub fn new(data: Arc<GameData>, sets: [Arc<Moveset>; 4]) -> Self {
        Self::with_teams(data, sets, vec![0, 0, 1, 1])
    }

    /// Four actors of `team`: two teams (0, 0, 1, 1) stand left and right facing each other; each for themselves
    /// (0, 1, 2, 3) they stand at the corners of a square facing its middle.
    pub fn with_teams(data: Arc<GameData>, sets: [Arc<Moveset>; 4], team: Vec<usize>) -> Self {
        let h = data.max_health * ONE;
        let fight = Fight::new(data, sets[0].clone(), sets[2].clone());
        let pairs = team.iter().max().copied().unwrap_or(0) <= 1;
        let actors = sets
            .into_iter()
            .enumerate()
            .map(|(i, m)| {
                let (x, z) =
                    if pairs { (if i < 2 { -1000.0 } else { 1000.0 }, if i % 2 == 0 { -500.0 } else { 500.0 }) } else { (if i % 2 == 0 { -900.0 } else { 900.0 }, if i < 2 { -700.0 } else { 700.0 }) };
                let yaw = if pairs { std::f32::consts::FRAC_PI_2.copysign(-x) } else { (-x).datan2(-z) };
                Fighter::new(m, h, Vec3::new(x, 0.0, z), yaw, if x < 0.0 { -100 } else { 100 })
            })
            .collect();
        let target = (0..4).map(|i| (0..4).map(|d| (i + 2 + d) % 4).find(|&j| team[j] != team[i]).unwrap_or(i)).collect();
        Self {
            fight,
            actors,
            team,
            target,
            locked: vec![false; 4],
            target_held: vec![false; 4],
            links: Vec::new(),
            clock: ROUND_FRAMES,
            state: MeleeState::Fighting,
            hits: Vec::new(),
            leash: Some(LEASH),
            behind_cost: BEHIND_COST,
            tag_pairs: false,
            kept: Vec::new(),
            objects: Default::default(),
        }
    }

    /// How many teams there are.
    pub fn teams(&self) -> usize {
        self.team.iter().max().map_or(0, |t| t + 1)
    }

    /// Actors as they stand (Force's co-op: the players and the soldiers), each of `team`; no leash. Each
    /// starts on the nearest foe.
    pub fn with_actors(data: Arc<GameData>, actors: Vec<Fighter>, team: Vec<usize>) -> Self {
        let fight = Fight::new(data, actors[0].moveset.clone(), actors[1].moveset.clone());
        let target = (0..actors.len()).map(|i| (0..actors.len()).find(|&j| team[j] != team[i]).unwrap_or(i)).collect();
        let n = actors.len();
        let mut m = Self {
            fight,
            actors,
            team,
            target,
            locked: vec![false; n],
            target_held: vec![false; n],
            links: Vec::new(),
            clock: ROUND_FRAMES,
            state: MeleeState::Fighting,
            hits: Vec::new(),
            leash: None,
            behind_cost: 0.0,
            tag_pairs: false,
            kept: Vec::new(),
            objects: Default::default(),
        };
        m.retarget();
        m
    }

    /// One more actor, of `team` (Force for more than two); it takes on the nearest foe.
    pub fn push_actor(&mut self, f: Fighter, team: usize) {
        self.actors.push(f);
        self.team.push(team);
        let i = self.actors.len() - 1;
        self.target.push((0..i).find(|&j| self.team[j] != team).unwrap_or(i));
        self.locked.push(false);
        self.target_held.push(false);
    }

    /// Actors `a` and `b` trade places (and with them their targets, the targets on them and their links).
    pub fn swap_actors(&mut self, a: usize, b: usize) {
        self.actors.swap(a, b);
        self.team.swap(a, b);
        self.target.swap(a, b);
        self.locked.swap(a, b);
        self.target_held.swap(a, b);
        let map = |i: usize| {
            if i == a {
                b
            } else if i == b {
                a
            } else {
                i
            }
        };
        for t in &mut self.target {
            *t = map(*t);
        }
        for l in &mut self.links {
            (l.0, l.1) = (map(l.0), map(l.1));
        }
    }

    pub fn standing(&self, i: usize) -> bool {
        self.actors[i].health > 0
    }

    /// The actor `i` is paired with in a throw, if any.
    pub fn linked_with(&self, i: usize) -> Option<usize> {
        self.links.iter().find_map(|&(a, b, _)| {
            if a == i {
                Some(b)
            } else if b == i {
                Some(a)
            } else {
                None
            }
        })
    }

    /// Who actor `i` faces this frame: its throw partner, else its target.
    pub fn facing(&self, i: usize) -> usize {
        self.linked_with(i).unwrap_or(self.target[i])
    }

    /// Whether actor `i` has its opponent to its right on the pad.
    pub fn facing_right(&self, i: usize) -> bool {
        (self.aim(i).0 - self.actors[i].root()).dot(self.fight.screen_right) >= 0.0
    }

    /// Where actor `i` turns (and how evasive that is): its opponent, or with none in play, Force's way on
    /// ([`Fight::idle_target`]).
    fn aim(&self, i: usize) -> (Vec3, u32) {
        let o = &self.actors[self.facing(i)];
        match self.fight.idle_target {
            Some(t) if o.inactive => (t, 0),
            _ => (o.root(), o.evasive),
        }
    }

    /// The targets picked now, as the next step picks them (from where everyone stands), and each actor that
    /// changed target turned to see the new one: the tag brawl's CPUs think before the step, so against the
    /// opponent the step fights and its pad's sides. Returns the actors that changed target.
    pub fn retarget_now(&mut self) -> Vec<usize> {
        let before = self.target.clone();
        self.retarget();
        let changed: Vec<usize> = (0..self.actors.len()).filter(|&i| self.target[i] != before[i]).collect();
        for &i in &changed {
            let o = self.facing(i);
            self.actors[i].opponent = sight_of(&self.actors[o], &self.fight.data);
        }
        changed
    }

    /// Each one sees whom it faces (before a round's first step).
    pub fn see_targets(&mut self) {
        for i in 0..self.actors.len() {
            let o = self.facing(i);
            self.actors[i].opponent = sight_of(&self.actors[o], &self.fight.data);
        }
    }

    /// How far actor `j` counts for actor `i` in the picks of a target: the distance, a foe behind counting
    /// further ([`Melee::behind_cost`]).
    pub fn pick_cost(&self, i: usize, j: usize) -> f32 {
        let (sin, cos) = self.actors[i].yaw.dsin_cos();
        let d = self.actors[j].root() - self.actors[i].root();
        let len = d.length();
        // (1 - cos of the angle off my facing) / 2: 0 in front, 1 straight behind
        let behind = if len > 1.0 { (1.0 - (d.x * sin + d.z * cos) / len) / 2.0 } else { 0.0 };
        len + self.behind_cost * behind
    }

    /// Actor `j` is a foe of actor `i` standing in play: one a lock holds on to.
    pub fn lockable(&self, i: usize, j: usize) -> bool {
        self.team[j] != self.team[i] && !self.actors[j].inactive && self.standing(j)
    }

    /// TARGET pressed by actor `i`: the target locked on (the one it fights, not yet locked), else the next foe in
    /// play by actor order.
    pub fn cycle_target(&mut self, i: usize) {
        let n = self.actors.len();
        let (cur, from) = (self.target[i], usize::from(self.locked[i]));
        if let Some(j) = (from..from + n).map(|d| (cur + d) % n).find(|&j| self.lockable(i, j)) {
            (self.target[i], self.locked[i]) = (j, true);
        }
    }

    fn retarget(&mut self) {
        for i in 0..self.actors.len() {
            if self.linked_with(i).is_some() {
                continue;
            }
            // (a lock holds while its foe stands in play; then the picks again, until TARGET again)
            if self.locked[i] {
                if self.lockable(i, self.target[i]) {
                    continue;
                }
                self.locked[i] = false;
            }
            let dist = |j: usize| self.pick_cost(i, j);
            // (actors out of play are nobody's target)
            let foes: Vec<usize> = (0..self.actors.len()).filter(|&j| self.team[j] != self.team[i] && !self.actors[j].inactive).collect();
            let standing: Vec<usize> = foes.iter().copied().filter(|&j| self.standing(j)).collect();
            let pool = if standing.is_empty() { &foes } else { &standing };
            let Some(&best) = pool.iter().min_by(|&&a, &&b| dist(a).total_cmp(&dist(b))) else { continue };
            let cur = self.target[i];
            if !pool.contains(&cur) || dist(best) + RETARGET_MARGIN < dist(cur) {
                self.target[i] = best;
            }
        }
    }

    /// Move actors `a`, `b` into the fight's slots 0, 1 (call again to move them back).
    fn swap_in(&mut self, a: usize, b: usize) {
        let [s0, s1] = &mut self.fight.fighters;
        std::mem::swap(s0, &mut self.actors[a]);
        std::mem::swap(s1, &mut self.actors[b]);
    }

    /// Run `f` on the pair `a`, `b` in the slots, with their link; keeps the link it leaves and the hits.
    fn on_pair(&mut self, a: usize, b: usize, f: impl FnOnce(&mut Fight)) {
        let k = self.links.iter().position(|&(x, y, _)| (x, y) == (a, b) || (x, y) == (b, a));
        self.fight.link = k.map(|k| {
            let (x, _, c) = self.links[k];
            if x == a {
                c
            } else {
                [c[1], c[0]]
            }
        });
        self.fight.hits.clear();
        self.swap_in(a, b);
        f(&mut self.fight);
        self.swap_in(a, b);
        let link = self.fight.link.take();
        match (k, link) {
            (Some(k), Some(c)) => self.links[k] = (a, b, c),
            (Some(k), None) => {
                self.links.remove(k);
            }
            (None, Some(c)) => self.links.push((a, b, c)),
            (None, None) => {}
        }
        let map = [a, b];
        for mut h in self.fight.hits.drain(..) {
            (h.attacker, h.victim) = (map[h.attacker], map[h.victim]);
            self.hits.push(h);
        }
    }

    /// One 60 Hz step: each actor's pad, collision volumes (last frame's pose) and animations.
    pub fn step(&mut self, pads: &[Pad], bodies: &[&BodyVolumes], anims: &[&dyn Animations]) {
        let n = self.actors.len();
        self.fight.frame += 1;
        // (whose strikes landed last step: the objects' fire is gone on a hit)
        let landed: Vec<bool> = (0..n).map(|k| self.hits.iter().any(|h| h.attacker == k)).collect();
        self.hits.clear();
        let gd = self.fight.data.clone();
        for i in 0..n {
            let held = pads.get(i).is_some_and(|p| p.buttons & crate::input::TARGET != 0);
            if held && !self.target_held[i] {
                self.cycle_target(i);
            }
            self.target_held[i] = held;
        }
        self.retarget();
        // Tekken Tag's pair distance limit (0x1D00) on each pair in play, before the moves as its fight's
        if self.tag_pairs {
            for a in 0..n {
                for b in a + 1..n {
                    if self.actors[a].inactive || self.actors[b].inactive {
                        continue;
                    }
                    let k = self.kept.iter().position(|e| (e.0, e.1) == (a, b));
                    self.fight.kept = k.map(|k| self.kept[k].2);
                    self.on_pair(a, b, |f| f.keep_together(&gd));
                    let kept = self.fight.kept.take();
                    match (k, kept) {
                        (Some(k), Some(c)) => self.kept[k].2 = c,
                        (None, Some(c)) => self.kept.push((a, b, c)),
                        _ => {}
                    }
                }
            }
        }
        for i in 0..n {
            let o = self.facing(i);
            let mut sight = sight_of(&self.actors[o], &gd);
            if let (Some(t), true) = (self.fight.idle_target, self.actors[o].inactive) {
                (sight.root, sight.position, sight.evasive) = (t, t, 0);
            }
            // a character the data marks cannot be thrown but by its own kind (0x800404E8)
            let c = self.actors[o].character;
            if c.is_some_and(|c| gd.characters.unthrowable.contains(&c)) && self.actors[i].character != c {
                sight.unthrowable = true;
            }
            let other = self.actors[o].moveset.clone();
            let f = &mut self.actors[i];
            f.opponent = sight;
            if f.current.bank != Bank::Partner {
                f.partner = Some(other);
            }
            // (Ball's back-steps end early, as in its pair fight)
            f.early_end = if self.fight.ball_rules { gd.ball.end(f.alias) } else { None };
        }
        let frozen: Vec<bool> = self.actors.iter().map(|f| f.freeze > 0).collect();
        let roots_before: Vec<Vec3> = self.actors.iter().map(Fighter::root).collect();
        // (each one's move frame before this step: a throw's victim that does not move on is held for good)
        let frame_before: Vec<u32> = self.actors.iter().map(|f| f.frame).collect();
        let fighting = self.state == MeleeState::Fighting;
        for i in 0..n {
            if self.actors[i].inactive {
                continue;
            }
            let facing_right = self.facing_right(i);
            let mut pad = if fighting && self.standing(i) { pads[i] } else { Pad::default() };
            // (no partner to tag)
            if self.tag_pairs {
                pad.buttons &= !crate::input::TAG;
            }
            self.actors[i].step(&gd, pad, facing_right, anims[i]);
        }
        // throws and holds: a link move pairs the actor with its partner (a thrown actor breaking free) or its
        // target. Two throws on one pair on the same frame: the frame's parity picks the winner (0x800455A0)
        let mut linking: Vec<(usize, usize)> = (0..n).filter(|&i| self.actors[i].started_with == Some(TRANSITION_LINK)).map(|i| (i, self.facing(i))).collect();
        let even = self.fight.frame.is_multiple_of(2);
        linking.retain(|&(i, j)| {
            let both = self.actors[j].started_with == Some(TRANSITION_LINK) && self.facing(j) == i;
            !both || self.actors[i].thrown || self.actors[j].thrown || (i < j) == even
        });
        for (i, j) in linking {
            self.on_pair(i, j, |f| f.link(&gd, 0, None));
        }
        // facing, after every actor's logic, against where its opponent is now
        for (i, &frozen) in frozen.iter().enumerate() {
            let (root, evasive) = self.aim(i);
            let seen = self.tag_pairs.then(|| [roots_before[i], roots_before[self.facing(i)]]);
            let f = &mut self.actors[i];
            (f.opponent.root, f.opponent.evasive) = (root, evasive);
            // (Tekken Tag: the pushback in the facing pass, the facing from the roots before the moves, a rebase at
            // once, as its pair fight's)
            if self.tag_pairs && f.tag_rules(&gd) {
                f.push_back();
            }
            if !frozen && !f.inactive {
                f.track(&gd, seen, false);
                if self.tag_pairs && f.rebase_root {
                    f.root_motion(&gd, anims[i]);
                }
            }
        }
        // the moves' damage to themselves (timed ops 0x0C / 0x0D), then the throws'
        for f in self.actors.iter_mut().filter(|_| !self.fight.ball_rules) {
            if f.self_damage != 0 {
                f.health = (f.health - f.self_damage * ONE).clamp(0, f.max_health);
            }
        }
        for (a, b, _) in self.links.clone() {
            self.on_pair(a, b, |f| f.throw_hits(&gd));
        }
        self.links.retain(|&(a, b, c)| {
            // (or one of them is out of play, a Force soldier gone: it steps no more, so never leaves its move)
            let over = (self.actors[a].current != c[0] && self.actors[b].current != c[1]) || self.actors[a].inactive || self.actors[b].inactive;
            // the thrower out of its throw while the one thrown, in the thrower's move, starts it again each frame
            // (from its first frame to its second and back): only the throw ends that move, so it stayed held, the
            // clock stopped, for good (seen in Force co-op's melee, never in a fight of two); the throw is over and
            // it stands again. A victim really in a throw plays its move on, past its second frame, and is left alone
            let left = [self.actors[a].current != c[0], self.actors[b].current != c[1]];
            let stuck = |k: usize| self.actors[k].current.bank == crate::data::Bank::Partner && !frozen[k] && frame_before[k] <= 1 && self.actors[k].frame <= 2;
            let held = [(b, left[0]), (a, left[1])].into_iter().find(|&(k, thrower_left)| thrower_left && stuck(k));
            if let (false, Some((k, _))) = (over, held) {
                self.actors[k].play(Current { bank: crate::data::Bank::Own, index: 0 });
            }
            let over = over || held.is_some();
            if over {
                for k in [a, b] {
                    self.actors[k].locked = false;
                    self.actors[k].thrown = false;
                }
            }
            !over
        });
        // the effect objects, seeing last step's contacts (as the pair fight's)
        crate::step_objects(&mut self.objects, &gd, &self.actors, bodies, anims, &landed, true);
        for f in &mut self.actors {
            (f.struck, f.whiffed, f.pushed, f.grabbed) = (false, false, false, None);
        }
        let free = |m: &Self, i: usize| !m.actors[i].inactive && m.linked_with(i).is_none();
        // (Tekken Tag's: the volumes in the pose of this frame; Tekken 3's the frame before's)
        let posed = |m: &Self| -> Vec<Option<BodyVolumes>> { (0..n).map(|i| if m.tag_pairs { anims[i].volumes(&m.actors[i], &gd) } else { None }).collect() };
        let now = posed(self);
        // each one's place of the last push, which Tekken Tag's push measures its step from: the same for each pair
        // shortcut: a second pair measures from there through the first pair's push, upgrade if a crowd pushes oddly
        let push_prev: Vec<Option<Vec3>> = self.actors.iter().map(|f| f.push_prev).collect();
        // body push between every pair outside a throw (team mates too)
        for a in 0..n {
            for b in a + 1..n {
                if free(self, a) && free(self, b) {
                    let pair = [now[a].as_ref().unwrap_or(bodies[a]), now[b].as_ref().unwrap_or(bodies[b])];
                    if self.tag_pairs {
                        (self.actors[a].push_prev, self.actors[b].push_prev) = (push_prev[a], push_prev[b]);
                        self.on_pair(a, b, |f| f.separate(pair));
                    } else {
                        self.on_pair(a, b, |f| f.push_apart(pair));
                    }
                }
            }
        }
        // the leash: back towards the group's centre (outside a throw)
        let active: Vec<usize> = (0..n).filter(|&i| !self.actors[i].inactive).collect();
        if let (false, Some(leash)) = (active.is_empty(), self.leash) {
            let centre = active.iter().map(|&i| self.actors[i].root()).sum::<Vec3>() / active.len() as f32;
            for &i in &active {
                let d = self.actors[i].root() - centre;
                let flat = Vec3::new(d.x, 0.0, d.z);
                let len = flat.length();
                if len > leash && self.linked_with(i).is_none() {
                    self.actors[i].position -= flat * ((len - leash) / len);
                }
            }
        }
        let now = posed(self);
        // hits between the teams; the frame's parity orders the pairs
        let mut pairs: Vec<(usize, usize)> = (0..n).flat_map(|a| (0..n).map(move |b| (a, b))).filter(|&(a, b)| a < b && self.team[a] != self.team[b]).collect();
        if !even {
            pairs.reverse();
        }
        for (a, b) in pairs {
            if !free(self, a) || !free(self, b) || (!self.standing(a) && !self.standing(b)) {
                continue;
            }
            let pair = [now[a].as_ref().unwrap_or(bodies[a]), now[b].as_ref().unwrap_or(bodies[b])];
            // (the two's striking objects, owned by their slots)
            let own = |o: &crate::objects::Object| [a, b].iter().position(|&k| k == o.owner).filter(|_| o.strikes).map(|owner| crate::objects::Object { owner, ..o.clone() });
            self.fight.objects.list = self.objects.list.iter().filter_map(own).collect();
            let slot = |o: usize| [a, b].iter().position(|&k| k == o);
            self.fight.objects.registered = self.objects.registered.as_ref().map(|r| r.iter().filter_map(|&(o, p, q)| Some((slot(o)?, p, q))).collect());
            self.on_pair(a, b, |f| f.resolve_hits(&gd, pair));
        }
        self.fight.objects.list.clear();
        if !gd.tag_moves {
            let gon: Vec<bool> = self.actors.iter().map(|f| f.character == Some(17)).collect();
            self.objects.on_hits(&self.hits, &gon);
        } else {
            self.objects.tag_hits(&self.hits);
        }
        if self.state == MeleeState::Fighting && self.links.is_empty() {
            self.clock = self.clock.saturating_sub(1);
            let teams = self.teams();
            let left: Vec<usize> = (0..teams).filter(|&t| (0..n).any(|i| self.team[i] == t && self.standing(i))).collect();
            if left.len() <= 1 {
                self.state = left.first().map_or(MeleeState::DoubleKo, |&w| MeleeState::Ko { winner: w });
            } else if self.clock == 0 {
                // the most health left wins (a tie at the top: nobody)
                let total = |t: usize| (0..n).filter(|&i| self.team[i] == t).map(|i| self.actors[i].health.max(0) as i64).sum::<i64>();
                let best = (0..teams).map(total).max().unwrap_or(0);
                let top: Vec<usize> = (0..teams).filter(|&t| total(t) == best).collect();
                self.state = MeleeState::TimeUp { winner: (top.len() == 1).then(|| top[0]) };
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// The game data and a model's move set for all four, or None without converted assets.
    fn melee() -> Option<Melee> {
        let root = std::env::var_os("OPENKUMITE_ASSETS").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets"));
        let g = root.join("gamedata");
        if !kumite_fs::exists(&g.join("movesets/index.json")) {
            eprintln!("skipping: no converted assets in {}", root.display());
            return None;
        }
        let data = Arc::new(GameData::load(&g).ok()?);
        let index: serde_json::Value = crate::data::load(&g.join("movesets/index.json")).ok()?;
        let file = index["models"][0]["moveset_file"].as_str()?;
        let set = Arc::new(crate::data::load::<Moveset>(&g.join("movesets").join(file)).ok()?);
        Some(Melee::new(data, [set.clone(), set.clone(), set.clone(), set]))
    }

    #[test]
    fn a_foe_behind_is_taken_on_only_when_clearly_nearer() {
        let Some(mut m) = melee() else { return };
        // actor 0 at the centre facing +x, its target (2) in front; its mate far off
        let face = |m: &mut Melee, i: usize, at: Vec3, yaw: f32| {
            (m.actors[i].position, m.actors[i].yaw, m.actors[i].root_yaw) = (at, yaw, yaw);
        };
        let east = std::f32::consts::FRAC_PI_2;
        face(&mut m, 0, Vec3::ZERO, east);
        face(&mut m, 1, Vec3::new(0.0, 0.0, 1900.0), east);
        face(&mut m, 2, Vec3::new(1000.0, 0.0, 0.0), -east);
        m.target[0] = 2;
        // the other foe behind, 500 nearer (past the retarget margin): the one in front stays
        face(&mut m, 3, Vec3::new(-500.0, 0.0, 0.0), east);
        assert!(!m.retarget_now().contains(&0));
        assert_eq!(m.target[0], 2);
        // ... right behind (950 nearer): him, and the actor sees him from then on
        face(&mut m, 3, Vec3::new(-50.0, 0.0, 0.0), east);
        assert!(m.retarget_now().contains(&0));
        assert_eq!(m.target[0], 3);
        assert_eq!(m.actors[0].opponent.root, m.actors[3].root());
        // the same foe 500 nearer in front: him at once
        m.target[0] = 2;
        face(&mut m, 3, Vec3::new(500.0, 0.0, 0.0), -east);
        assert!(m.retarget_now().contains(&0));
        assert_eq!(m.target[0], 3);
    }

    #[test]
    fn target_locks_cycles_by_actor_and_lets_go() {
        let Some(m) = melee() else { return };
        let sets = std::array::from_fn(|i| m.actors[i].moveset.clone());
        // each for themselves, all three foes in front of actor 0 in a row
        let mut m = Melee::with_teams(m.fight.data.clone(), sets, vec![0, 1, 2, 3]);
        let east = std::f32::consts::FRAC_PI_2;
        for (i, f) in m.actors.iter_mut().enumerate() {
            (f.position, f.yaw, f.root_yaw) = (Vec3::new(1000.0 * i as f32, 0.0, 0.0), east, east);
        }
        m.retarget_now();
        assert_eq!((m.target[0], m.locked[0]), (1, false), "the nearest, picked");
        // the first press locks the one fought; the next ones go round the others by actor
        let mut seen = Vec::new();
        for _ in 0..4 {
            m.cycle_target(0);
            seen.push(m.target[0]);
        }
        assert_eq!(seen, [1, 2, 3, 1]);
        assert!(m.locked[0]);
        // held while it stands, however near another comes
        m.cycle_target(0);
        m.actors[1].position.x = 100.0;
        m.retarget_now();
        assert_eq!((m.target[0], m.locked[0]), (2, true));
        // down: the picks again (the nearest), until TARGET again; one out of play is passed over
        m.actors[2].health = 0;
        m.retarget_now();
        assert_eq!((m.target[0], m.locked[0]), (1, false));
        m.actors[3].inactive = true;
        m.cycle_target(0);
        m.cycle_target(0);
        assert_eq!((m.target[0], m.locked[0]), (1, true), "the only foe left in play");
        // two teams: never a team mate
        let mut t = Melee::with_teams(m.fight.data.clone(), std::array::from_fn(|i| m.actors[i].moveset.clone()), vec![0, 0, 1, 1]);
        for _ in 0..4 {
            t.cycle_target(0);
            assert_eq!(t.team[t.target[0]], 1);
        }
    }
}
