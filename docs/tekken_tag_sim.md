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
- The tag battle (sections 6, 10, 15; `Fight::start_tag`, `TagTeam`, `TagMember`): two members a side, the benched
  one only its team entry. The tag button (command bit 14, kept as `input::TAG_COMMAND`; the pad's bit 4,
  `input::TAG`, read only with a partner) starts the tag-out run (5398 / 5394); its END or condition 0x5C (off
  screen) goes to 5397 with the swap bit (0x2000 without a parameter: `TRANSITION_SWAP`); the swap brings the partner in
  with its health on its own 5397 at the leaving fighter's root, facing the opponent, and an airborne opponent's
  next hit counts as a first air hit (the count at -1: `Fighter::juggle_bias`). No invulnerability. Recoverable
  health: a hit lowers the victim's cap by 6 / 10 of the damage (14 / 10 down to the health on a first air hit); the
  benched partner regains max / 6000 a frame up to it. Anger: 4 a hit on the active member to the partner (6 without
  damage), one more / less / none by the 76 relations (rules.json `tag`, the converter's tag export); at 120 the
  member deals x1.3 while in, draining 1 a frame. Round end: the active member's K.O.; at time up the higher average
  life ratio. Partner gates: `req` 0x64-0x84 (partner's moveset), 0xA7-0xC8 (its body), 0xEC / 0xED (the
  Kazuya-Devil pair, bodies 19 / 29, or not); the partner bit (cond 0x80) against the opponent's tagging (+0x129).
  Test: `kumite_game --test tekken_tag` (`tag_out_*`, `recoverable_health_and_anger`, `a_team_loses_*`,
  `a_partner_move_*`).
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
- Gate 0xEB never passes.
- Tag: the off-screen test uses the pelvis's projection on a 4:3 view, not three bones'; the opponent's tagging
  (+0x129) is its screen side now, not at its tag move's start; the relations take every fighter as variant 0
  (Kuma, not Panda); a throw's damage does not lower the recoverable cap; tag + a button takes the button held.
  The fight camera is Tekken 3's: it frames the runner (the run often ends on its END, not off screen), and pulled
  far out it can go into an arena's scenery.

## Missing

- `special_8003` (Unknown's moveset change), the solo Unknown's recovery; tag throws and tag combos beyond the partner
  gates (none found in code); the round's poses as a pair (the partner joining the winner's pose).
- Conditions 0x26 / 0x27 (unused) and 0x48-0x5B, 0x5D-0x5F (camera-relative, screen sides, round decided: the tag-out
  runs' variants and one entry): never true. The prefilter is not used (the full test decides).
- The counter hit caused by victim +0x12E.
- Other property ops (hand poses, sounds' numbers, camera scripts); air recovery and the missing-move fallback
  alias are unknown.
- Ogre's bone 0x13 and projectile bones 0x18 / 0x19 give no strike segment.
- The AI (no ai.json: a CPU does little), the round-start moves (the fighters stand), the losers' end motions.

## The game

`kumite_game::Game::load` builds a Tekken Tag fight from its pack: the fighters with their motions
(`movesets/motions_NN.json`), movesets through the dialect, hurt and push spheres from rules.json's tables on the posed
skeleton, the 18 bone slots (`BONE_SLOTS`) for the strikes, the motions' root translation. `openkumite --assets
<pack>/assets --fight --models 345,372 --stage 0` (or Tekken Tag the main game) plays a match of rounds for two
pads, with a plain HUD (`hud::plain`: no screens.json); `--models 345,372,357,465` a tag battle (Paul and Law against
Lei and Jin: the third and fourth are the partners), the HUD with each side's partner bar and its recoverable part
(red). The tag button: keyboard H / L (P2 backslash / numpad 6), gamepad R1, button 5 in pad scripts. Test: `OK_TTT_ASSETS=<pack>/assets cargo test --release -p
kumite_game --test tekken_tag`.

Provisional: the camera (`CameraData::provisional`, Tekken 3's constants; no camera.json), two rounds to win, the heads
do not turn to the opponent. Not there: sounds and music, hit sparks, shadows, menus, the browser build.
