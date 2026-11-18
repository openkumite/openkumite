# Tekken Tag Tournament on the simulation

`kumite_sim::dialect` loads a Tekken Tag `gamedata/` folder (ok_tekken_tag_converter, `rules.json` `"dialect":
"ttt"`) into the engine's `GameData`, rewriting Tekken Tag's numbers into the engine's (Tekken 3's) once at load.
Tekken 3's data never goes through it. The whole move and cancel tables are the common set; a fighter's moveset
(`dialect::moveset`) is its aliases into it. Test: `OK_TTT_ASSETS=<pack>/assets cargo test --release -p kumite_sim
--test tekken_tag` (a launcher and juggle, a counter hit, a back-turned attack, Paul against Jin); the code tables:
`cargo test -p kumite_sim dialect`. References: the converter's docs/FIGHT_DATA.md sections 7-12.

## Mapped

- Conditions (section 7): 0-0x24 kept; 0x25 "not back turned" (the engine's own code `COND_NOT_BACK`); 0x28-0x47
  are Tekken 3's 0x25-0x44; reversals 0x60-0x66 (0x45-0x4B); the family reversal's parameter (an offset) becomes the
  family's index; motion tags are the motion numbers. The partner bit (0x80) tests the opponent's tagging, which
  one on one never happens: the code alone decides.
- `req` gates: own / opponent's moveset, own body, widened to `kind << 8 | value` (one new arm in `passes_gate`).
- Transition types (section 8, type = bits 0-6): 0-28 kept; 29 / 30 Tekken 3's 0x1D / 0x1E (30 flies to air entry
  4's height); 31 / 32 the throw link; 33 a restart facing the opponent in mode 8 (`TRANSITION_FACE`); 34 / 35 a
  turn over 8 frames, at most 10 degrees a frame, to the screen axis or across it (`TRANSITION_AXIS`,
  `TRANSITION_ACROSS`); 36-45 Tekken 3's 0x22-0x2B; 46 use END; the turn bit 0x8000 and bit 0x4000.
- Commands, sequences, dash, backdash, shared lists (call / return).
- Flag bits (unthrowable, not reversible, guard by input, ki charge, root commit and the others with a Tekken 3
  pair).
- Timed property ops 1-10, damage to the opponent (0x0C) and to self (0x0D).
- The aliases the code names (`Moveset.fixed`): stance 58, crouch 80, K.O. collapse 4769.
- Hits: the chain's unconditional record (damage, type, reaction); the close reaction and distance on the engine's
  close-range table (1.5x damage, as Tekken Tag's code); "none" (0x592).
- Throws: a link move's reaction names the victim's alias in its first slot.
- Reactions and pushback curves (by index); health = the default life option (3: 140) x the body's percent
  (`GameData::max_health_of`).
- Juggles (section 9): the 44-entry air table (`reactions.json` `air_reactions`) and each move's own entry
  (`Move::air_reaction`, record +0x2F); Tekken Tag's choice (`GameData::tag_juggles`, 0x374048): face-down and
  early-getup victims 7 / 5 / 6, the move's own entry, spike 1, the launch fall 0, side hits 2 / 3, else 0;
  80 % on the first air hit, 50 % after (`rules.json` `juggle`).
- Damage: no damage zones; counter hits D x 12 / 10 + 3 / 10 of the victim's move damage x frame / startup
  (`GameData::counter_bonus`, Tekken 3's [3, 3]); chip D / 10 (`GameData::chip_add` 0, Tekken 3's 2).
- Strikes as bone-pair segments (`Attack.pairs`, `BodyVolumes.bones`; the pose's slots come from the app).

## Approximated

- Hit chains: the gates (by the opponent's moveset) are not tried.
- Transitions 34 / 35: the pair's line stands in for the camera's screen axis (the fight camera looks across it).
- Air reactions: every air hit takes entry 0's pushback samples (the moves' own entries have their own); the
  face-down victims' height test uses the root's height for +0x520; the entries' fixed rise (400 on spikes) is
  the engine's spike speed, the same value.
- A counter hit from the side or back takes the counter slot (Tekken Tag: the side / back slot).
- Pushback curves keep 8 of their 10 impulses (the last two are zero on the USA disc).
- Gate 0xED (not the Kazuya-Devil pair) passes, 0xEB / 0xEC never.

## Missing

- Tag: the tag button (command bit 14), partner gates, `special_8003`, the swap, recoverable health, anger
  (section 10): never match / not run.
- Conditions 0x26 / 0x27 (unused) and 0x48-0x5F (camera-relative, screen sides, off screen, round decided: used by
  the tag-out runs and one entry): never true. The prefilter is not used (the full test decides).
- The counter hit caused by victim +0x12E; the 1.3x partner-anger damage; recoverable-cap loss.
- Other property ops (hand poses, sounds' numbers, camera scripts); air recovery and the missing-move fallback
  alias are unknown.
- Ogre's bone 0x13 and projectile bones 0x18 / 0x19 give no strike segment.
- The AI (no ai.json: a CPU does little), the round-start moves (the fighters stand), the losers' end motions.

## The game

`kumite_game::Game::load` builds a Tekken Tag fight from its pack: the fighters with their motions
(`movesets/motions_NN.json`), movesets through the dialect, hurt and push spheres from rules.json's tables on the posed
skeleton, the 18 bone slots (`BONE_SLOTS`) for the strikes, the motions' root translation. `openkumite --assets
<pack>/assets --fight --models 345,372 --stage 0` (or Tekken Tag the main game) plays a match of rounds for two
pads, with a plain HUD (`hud::plain`: no screens.json). Test: `OK_TTT_ASSETS=<pack>/assets cargo test --release -p
kumite_game --test tekken_tag`.

Provisional: the camera (`CameraData::provisional`, Tekken 3's constants; no camera.json), two rounds to win, the heads
do not turn to the opponent. Not there: sounds and music, hit sparks, shadows, menus, the browser build.
