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
  gates (none found in code).
- Conditions 0x26 / 0x27 (unused) and 0x48-0x5B, 0x5D-0x5F (camera-relative, screen sides, round decided: the tag-out
  runs' variants and one entry): never true. The prefilter is not used (the full test decides).
- The counter hit caused by victim +0x12E.
- Other property ops (hand poses, sounds' numbers, camera scripts); air recovery and the missing-move fallback
  alias are unknown.
- Ogre's bone 0x13 and projectile bones 0x18 / 0x19 give no strike segment.
- The AI (no ai.json: a CPU does little), the losers' end motions, the team-lose shot (arcade).

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
do not turn to the opponent. Not there: hit sparks, shadows, menus, the browser build.

## Round flow and sound

Round flow (`bout::RoundRules`, rounds.json; FIGHT_DATA.md section 13): the start motions (aliases 4 / 6 by □ △ / × ○
held else a coin flip, later rounds 5 from frame 1), the fight on the call's frame 151 / 91, the K.O. replay from frame
16 played twice, a time up done on frame 160, the win pose 4770-4773 (held button else the frame parity) for 300
frames (START from frame 2), a tag battle's winner's partner posing the same alias in the loser's place
(`Game::pose_partner`, x 0x400 z -0xC00 taken as right of and behind the winner), the 60 s clock shown in seconds of 70
frames, the plain HUD's ROUND n / READY! / K.O. / TIME UP / PERFECT / GREAT. Guessed: which pad bits are which
buttons (Tekken 3's order), FIGHT! shown 40 frames. Not done: the replay's speed (Tekken 3's half), frames 17-135
after the replay, the time-up winner's motion, the arcade's team-lose shot.

Sound (`sound::Tag`, sound.json's dialect; section 14): ids through the 62 groups, voices from the character file's
bank and row, played at pitch x 48000 / 4096; voices (types 2 / 3, the row's cooldown), the K.O. voice, hit sounds on
the connecting frame (guard 0x10, the generic ids by damage band and end bone, the four bodies' arm table, the K.O.'s
0x0A / 0x0D), timed scripts (kinds 0-2; 0x13 a footstep pair 4 frames apart on the arena's floor), the jump and the
arena's landing; the announcer (ids 0x48-0x5C as the round code plays them, which the samples' names confirm: count1-8,
time_up, great, perfect, ko1, round, fight; 0x5C, played as FINAL ROUND, is named TagBattle); the arena's BGM, looped
from the start. Not done: type 9 and script kind 3 (0x376578), the Jacks' and Mokujin's own hit handlers, the samples'
volume byte, the music's loop point, YOU WIN / LOSE. Tests: `kumite_game --test tekken_tag`. A video: `--fight
--models ... --export-video fight --frames N --out F.mp4`.

## Front end

With a pack whose roster.json has `front_end` (converter format 8; `app/src/ttt.rs`), Tekken Tag gets its own front
end in place of the `--fight` sandbox; an older pack keeps the plain one. Title (`134_003`, PRESS P1 START), the main
menu 0x51EFE0 (ARCADE, VS BATTLE, TEAM BATTLE, TIME ATTACK, SURVIVAL, 1 ON 1, 1 ON 1 VS, PRACTICE, OPTIONS; THEATER
once an ending was seen; TEKKEN BOWL once the unlock stage reaches the schedule's kind 5, 10; the locked GALLERY not
shown), the select grid 0x440A68 (faces at (x, y + 52), locked faces by the unlock
stage, RANDOM; tag modes pick two, 1 ON 1 one, Team Battle up to eight with START ending, 2P's START joins), a VS
screen, the fight with the HUD from the art (the six strips, angry strips, name plates on their streaks, round lamps,
the timer's digits, STAGE n / FINAL, the partner's bar), CONTINUE? 9..0 (898 frames, a button skips 100), GAME OVER,
OPTIONS (DIFFICULTY LEVEL, FIGHT COUNT, ROUND TIME: kept in the save). Arcade, Time Attack and 1 ON 1: eight fights,
1-6 from the CPU pool (one a pair of 0x5207F0), 7 the rival team in Ogre's arena, 8 Unknown alone, one round, in hers.
Team Battle: one match a member, the winner stays with 15 % back. Survival: one round a fight, the health carried.
The save (`openkumite_save_ttt.json`, `ttt`): the unlock stage (0x440818: a clear with a new ending or 50 matches
advance it), points, the endings seen (`seen`, a bit per ending), options.

Endings (a pack of converter format 10: `gamedata/endings.json`, `endings/`; `kumite_game::ending`): an Arcade, Time
Attack or 1 ON 1 clear picks the front member's ending by costume (0x51B678), the partner's when the front's was seen
and the partner's was not (0x3AA9D4), and plays the staff roll alone for 4620 frames (BGM 6; the 434 lines of
0x51B880 rising 1.5 px a frame, 32 px apart; START skips it), then the ending's scene under it, then the roll to its
end. The scene is real time: its arena, two actor slots the script fills from the cast (the player's costume where
the cast says -1), each slot's body track (the root's track places it), the camera path (H on Tekken Tag's 512-pixel
screen, as the fight camera's), the script's fades from and to black, white flashes and fade to white; every record
runs once its frame has come, the tracks and the camera are sampled at the camera's frame, so a frame depends on its
number alone. Unknown's ending (a movie) gives the roll alone. THEATER (0x3544C8): a row per character with an ending
seen, a cell per labelled costume whose ending was seen (the scene with BGM 7, no roll), then OPENING MOVIE, ARCADE
MOVIE and EMBU (shown, not playable), MUSIC PLAYER (0x455D80's BGM list), EXIT.

OpenKumite's (not decoded): the menus' and the select's layout below the grid (portraits of files 143-181, the backdrop
of file 182 / 141), the VS screen, which button gives which costume (1-4, START the fifth), which strip is which,
the partner bar's place, the banners in the chrome font `003_001`, a play point a match, the Mokujin / Unknown slots
shown once unlocked, Theater's layout, the staff roll's fonts (egui's) and its logo cards. Not done:
Gallery, the records, the select timer, the help lines, Practice's own menus (a free fight), replays of a tag battle;
in the endings, the effect ops (sparks, objects, show / hide), the face and hand tracks, the arenas' background
characters, BGM 7's start 134.87 s into its stream (the Theater's ending music starts at the stream's start); the
movies (MPEG-2, not converted) and the EMBU exhibition.

## Tekken Bowl

Converter format 11 (`gamedata/bowl.json`, `motions/bowl_NNN.glb`, `stages/bowl_ball.glb` / `bowl_pin.glb`; its
docs/BOWL_ENDINGS.md sections 1-6). `kumite_sim::bowl` is the game, deterministic (f32 with reproducible operations,
the trigonometry through `detmath`, the game's random draws from a seed; `Bowl::hash`): the state machine 0x3F0008,
the throw's phases (position +-8 to +-1000, aim +-0x10 to +-0x800 with the spin meter swinging within the hand's
bounds, the power meter wrapping at 100, CROSS on the wrap frame OVER CHARGED with probability S / 2000), the run-up
(steering 1280 / S), the release at the throw motion's frame 106 (speed S (power + 100) / 200, the meters off by their
own speeds), the ball (fall, bounce, hook rescaled to its speed, the deck walls), the 25 spectators (a K.O. within 300),
the end of a roll, the count, ten-pin scoring with STRIKE / SPARE / MISS / FOUL / SPLIT, the callouts (DOUBLE!!,
TURKEY!!!, !!GREAT!!, PUNCH OUT, PERFECT, NICE COVER) and their announcer, the pin clatter, OVER CHARGED's charge (the
runner's body against the pins, a FOUL). `kumite_game::bowl::BowlScene` puts the characters on it: the bowl motions by
hand (stand, steps, throw, the charges by style, the strike reaction), the hand's release point at frame 106 and the
charge's travel from the motions, the camera. The app (`App::start_bowl`): TEKKEN BOWL's select (P1 picks; 2P's START
joins; Mokujin and Unknown bowl with a random character's stats), the hall (arena 20), the ball, the pins and the pin
deck drawn with the hall's model (the arena objects the roll's draw 0x3EEF00 names, placed by its object table), the HUD (`ttt::bowl_hud`), the results, back to the menu. Keys: left / right, CROSS (button 3),
CIRCLE (button 4) back to the position, START skips a strike's pose; BACK leaves. Picture: `--screen bowl [--variant
2]` plays scripted throws. Tests: `kumite_sim` `bowl::tests` (scoring, the tenth frame, 300, a straight throw, the
hook by hand, OVER CHARGED's FOUL, a K.O., determinism); `kumite_game --test tekken_tag tekken_bowl_from_the_pack`.

OpenKumite's (not decoded): the pins' collision response (three spheres and a base each, impulses with friction;
the original's 0x3EDC90 / 0x3EDF88), the HUD's layout and gauges (plain boxes, the front end's fonts), the cameras'
offsets from their decoded bases, the strike pose's length (150 frames), the count's callout time. Not done: the
spectators drawn (their K.O. is tested, not shown), the floor between the pins and the deck (the hall has a hole
there: what the original draws in it is not found), the ball's and pins' shadows (objects 2, 3), the strike win poses 4770-4773 and the partner's, the replay, the BOWL STYLE / SWITCH HANDS menu, the pause
menu, the records (THE KING OF IRONBOWL) and the 200 / 300 unlocks, the aiming guide.
