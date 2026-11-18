# Tekken Tag Tournament on the simulation

`kumite_sim::dialect` loads a Tekken Tag `gamedata/` folder (ok_tekken_tag_converter, `rules.json` `"dialect":
"ttt"`) into the engine's `GameData`, rewriting Tekken Tag's numbers into the engine's (Tekken 3's) once at load.
Tekken 3's data never goes through it. The whole move and cancel tables are the common set; a fighter's moveset
(`dialect::moveset`) is its aliases into it. Test: `OK_TTT_ASSETS=<pack>/assets cargo test --release -p kumite_sim
--test tekken_tag`.

## Mapped

- Condition codes 0-43, 47-71 (Tekken 3's 44-68), reversals 0x60-0x66 (0x45-0x4B); the family reversal's
  parameter (an offset) becomes the family's index; motion tags are the motion numbers.
- `req` gates: own / opponent's moveset, own body, widened to `kind << 8 | value` (one new arm in `passes_gate`).
- Transition types 0-28, 32 (throw link), 46 (use END); the turn bit 0x8000 and bit 0x4000.
- Commands, sequences, dash, backdash, shared lists (call / return).
- Flag bits (unthrowable, not reversible, guard by input, ki charge, root commit and the others with a Tekken 3
  pair).
- Timed property ops 1-10, damage to the opponent (0x0C) and to self (0x0D).
- The aliases the code names (`Moveset.fixed`): stance 58, crouch 80, K.O. collapse 4769.
- Hits: the chain's unconditional record (damage, type, reaction); the close reaction and distance on the engine's
  close-range table; "none" (0x592).
- Throws: a link move's reaction names the victim's alias in its first slot.
- Reactions and pushback curves (by index); health = life option x the body's percent (`GameData::max_health_of`).
- Strikes as bone-pair segments (`Attack.pairs`, `BodyVolumes.bones`; the pose's slots come from the app).

## Approximated

- Hit chains: the gates (by the opponent's moveset) are not tried.
- The close reaction also gets Tekken 3's 1.5x damage.
- Other transition types (29-31, 33-45, bits 6-13) are a plain restart.
- The default life option (taken as 130), juggle scaling (Tekken 3's 80 / 50 %).
- Pushback curves keep 8 of their 10 impulses (the last two are zero on the USA disc).
- Gate 0xED (not the Kazuya-Devil pair) passes, 0xEB / 0xEC never.

## Missing

- Tag: the tag button (command bit 14), partner conditions (bit 0x80), partner gates, `special_8003`: never match.
- Conditions 44-46 and 0x48-0x5F (not decoded): never true. The prefilter is not used (the full test decides).
- Juggles: no air reaction table was found, so airborne hits get no air reaction; air recovery and the missing-move
  fallback alias are unknown.
- Damage zones, counter-hit scaling, other property ops (hand poses, sounds' numbers, camera scripts).
- Ogre's bone 0x13 and projectile bones 0x18 / 0x19 give no strike segment.
- The game layer (`kumite_game::Game`, bouts, win poses 4770-4773, AI) does not load Tekken Tag yet.
