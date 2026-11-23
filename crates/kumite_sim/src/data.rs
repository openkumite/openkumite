//! Typed views of the converted game data (`assets/gamedata/*.json`, written by export_gamedata).
//! Only the fields the simulation uses are declared; serde ignores the rest.

use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::Path;

/// How many moves a moveset file has (its moves skipped over, not built).
pub fn move_count(path: &Path) -> Result<usize> {
    #[derive(Deserialize)]
    struct Count {
        moves: Vec<serde::de::IgnoredAny>,
    }
    Ok(load::<Count>(path)?.moves.len())
}

pub fn load<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    let b = kumite_fs::read(path).with_context(|| format!("opening {}", path.display()))?;
    serde_json::from_slice(&b).with_context(|| format!("parsing {}", path.display()))
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Bank {
    Own,
    Common,
    None,
    /// A move of the opponent's set, played on this fighter: throw victims play the thrower's victim
    /// animations (the game points the thrown actor at the thrower's records). Never in the data files.
    Partner,
}

/// A move reference: which moveset (the character's own, or the shared common set) and the record index.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Hash)]
pub struct MoveRef {
    pub bank: Bank,
    #[serde(rename = "move")]
    pub index: Option<usize>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Moveset {
    /// Moveset id (descriptor +9); `None` for the common set.
    pub moveset_id: Option<i64>,
    pub moves: Vec<Move>,
    pub cancels: Vec<Cancel>,
    /// Alias (move id) table: 4023 entries in a character's moveset; absent in the common set.
    #[serde(default)]
    pub aliases: Option<Vec<MoveRef>>,
    /// Scripted camera motions, by camera selector index (decoder 0x80038CE4).
    #[serde(default)]
    pub camera_motions: std::collections::HashMap<String, CameraMotion>,
    /// The aliases the engine plays by number (Tekken 3's unless a dialect sets its own, `dialect.rs`).
    #[serde(skip)]
    pub fixed: Aliases,
}

/// Aliases the code names (kumite_converter's reference engine.md §7.1).
#[derive(Clone, Debug)]
pub struct Aliases {
    /// The stance, and the crouch (a K.O. reaction going back to either collapses at once).
    pub neutral: usize,
    pub crouch: usize,
    /// The K.O. collapse (common_219, 80 frames).
    pub ko_collapse: usize,
    /// The alias both fighters fall back to for a move neither set has (move-properties.md §3.1).
    pub missing: usize,
    /// Air reactions (0x8002EE94): the late fall, the two side falls, the air recovery.
    pub air_late: usize,
    pub air_side: [usize; 2],
    pub air_recovery: usize,
    /// The round-start moves (0x8002D31C): round 1's by a held □ / △ and by ×, later rounds' (Tekken Tag's are not known).
    pub round_start: [usize; 3],
}

impl Aliases {
    pub const TEKKEN3: Self = Self { neutral: 3, crouch: 0x28, ko_collapse: 0xD66, missing: 0xDC5, air_late: 0xD62, air_side: [0xD63, 0xD64], air_recovery: 0xD58, round_start: [0, 2, 1] };
}

impl Default for Aliases {
    fn default() -> Self {
        Self::TEKKEN3
    }
}

impl Moveset {
    /// The move references in range: some moves, each alias's own move in this set, its common one in the common set
    /// (`common` moves). A fighter indexes its moves by them every frame.
    pub fn check(&self, common: usize) -> Result<()> {
        anyhow::ensure!(!self.moves.is_empty(), "no moves");
        for (k, r) in self.aliases.iter().flatten().enumerate() {
            let len = match r.bank {
                Bank::Own => self.moves.len(),
                Bank::Common => common,
                _ => continue,
            };
            anyhow::ensure!(r.index.is_none_or(|i| i < len), "alias {k}: {:?} move {:?} past the {len} moves", r.bank, r.index);
        }
        Ok(())
    }
}

/// A scripted camera motion: per frame, its channels (mode 1: eye x y z, target x y z, ×2; game space).
#[derive(Clone, Debug, Deserialize)]
pub struct CameraMotion {
    pub frames: usize,
    pub values: Vec<Vec<i32>>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Move {
    pub index: usize,
    /// glTF animation name in the character's .glb (`motion_NNN` or `common_NNN`).
    pub anim: String,
    pub frames: u32,
    pub posture: Posture,
    pub attack_mask: AttackMask,
    pub cancel_start: usize,
    /// The motion's family tag (reversals, cond 0x4B).
    #[serde(default)]
    pub motion_tag: i32,
    pub next: Next,
    pub damage: i32,
    pub damage_type: u32,
    /// Record +0x16: the drift along the facing over the jump window (0x8003F3A8), per frame / 2048 units.
    #[serde(default)]
    pub air_drift: i32,
    /// First and last active frame (0, 0 = no attack).
    pub active: [u32; 2],
    #[serde(default)]
    pub hitstop: u32,
    /// Tekken Tag's record +0x2F: the air reaction this move's juggle hits take (8-43; 0 none, and in Tekken 3).
    #[serde(default, alias = "byte_2F")]
    pub air_reaction: usize,
    pub reaction: Reaction,
    #[serde(default)]
    pub attack: Option<Attack>,
    #[serde(default)]
    pub properties: Option<Vec<Property>>,
    #[serde(default)]
    pub flags24: Flags24,
    /// Record +0x30: the push spheres' sizes (Tekken Tag's 0x377B58).
    #[serde(default)]
    pub push_control: PushControl,
    /// Voice/SFX event list (move-properties.md §5).
    #[serde(default)]
    pub sounds: Option<Vec<SoundEntry>>,
    /// Close-range reaction (record +0x34; index into reactions.json `close_range`, 0 = none).
    #[serde(default)]
    pub close_range: usize,
    #[serde(default)]
    pub bytes_18: Bytes18,
    /// Attack descriptor byte 0 (record +0x28, built-in descriptors included): the CPU AI tests the opponent's
    /// (0x18 and 0x1A: the two characters the data names).
    #[serde(default)]
    pub desc0: Option<u32>,
}

/// One sound-list entry: `type << 12 | index`.
#[derive(Clone, Debug, Deserialize)]
pub struct SoundEntry {
    #[serde(rename = "type")]
    pub kind: u32,
    pub index: u32,
}

/// Move record +0x30: low 4 bits a mode of the push spheres' sizes; with mode 0, bits 4-11 drop spheres (bit 12: only
/// from 8 frames before the active frames to their end).
#[derive(Clone, Debug, Default, Deserialize)]
pub struct PushControl {
    pub raw: u32,
}

/// Move word +0x24: flag bits plus the camera id byte (0x27..0x2A cap the fight camera's yaw speed).
#[derive(Clone, Debug, Default, Deserialize)]
pub struct Flags24 {
    pub raw: u32,
    pub camera_script: u32,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Posture {
    pub raw: u32,
    pub base: u32,
}

#[derive(Clone, Debug, Deserialize)]
pub struct AttackMask {
    /// Full word: hit mask in the low bits, CPU AI range/class hints from bit 16 (0x40000 close, 0x80000 mid,
    /// 0x100000 long, 0x400000 throw).
    #[serde(default)]
    pub raw: u32,
    pub mask: u32,
    pub level: u32,
    pub level_name: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Next {
    pub alias: usize,
    /// Record +0x12: the animation's net turn (u16, 0x10000 = 360°), added to the facing by END / buffered links.
    #[serde(default)]
    pub hi: u32,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Reaction {
    pub index: usize,
    /// "reactions" (the 14-slot table) or "throw_victims" (id & 0x1000: the victim alias list 0x800174C4).
    #[serde(default)]
    pub table: String,
}

impl Reaction {
    pub fn is_throw(&self) -> bool {
        self.table == "throw_victims"
    }
}

/// Record bytes 0x18..0x1B: the jump window and the frame a fall animation holds until landing.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct Bytes18 {
    /// Record +0x18: the head may follow an attack from frame (this − 17) on (0x80039104).
    #[serde(default)]
    pub frame_count_ram: u32,
    #[serde(default)]
    pub jump_start: u32,
    #[serde(default)]
    pub jump_end: u32,
    #[serde(default)]
    pub hold_frame: u32,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Property {
    pub frame: u32,
    pub op: u32,
    pub arg: u32,
}

/// Baked strike path: per active frame, up to two segments in character space (original-hardware axes, game units).
#[derive(Clone, Debug, Deserialize)]
pub struct Attack {
    #[serde(default)]
    pub frames: Vec<AttackFrame>,
    /// Descriptor bytes 0..5: the striking limbs' bone slots first (reversal limb test 0x8002EB4C).
    #[serde(default)]
    pub header_bytes: Vec<u32>,
    #[serde(default)]
    pub bones: Option<AttackBones>,
    /// Tekken Tag's strike (no baked path): up to two (start, end) bone slots, posed every frame (`dialect.rs`).
    #[serde(default)]
    pub pairs: Vec<[u32; 2]>,
}

/// Bone slots of the strike path (end0 < 0xC = an arm, else a leg; used to pick block sounds).
#[derive(Clone, Debug, Deserialize)]
pub struct AttackBones {
    pub end0: u32,
}

#[derive(Clone, Debug, Deserialize)]
pub struct AttackFrame {
    pub frame: u32,
    pub segments: Vec<[[i32; 3]; 2]>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum CancelKind {
    Command,
    Sequence,
    End,
    Call,
    Return,
    Dash,
    Backdash,
    #[serde(other)]
    Other,
}

/// A command/cancel entry (12 bytes in the game; kumite_converter's reference engine.md §7.2).
#[derive(Clone, Debug, Deserialize)]
pub struct Cancel {
    pub cmd: u32,
    pub kind: CancelKind,
    #[serde(default)]
    pub input: String,
    pub req: u32,
    pub cond: u32,
    /// Tekken Tag's partner bit (cond 0x80): also test the opponent's tagging ([`crate::Fighter::tag_test`]).
    #[serde(default)]
    pub cond_tag: bool,
    pub param: u32,
    pub dest: usize,
    pub win_start: u32,
    pub win_end: u32,
    /// Frame at which the buffered move takes over (255 = at animation end); for `Call`, unused.
    pub exec: u32,
    /// Transition byte; 0x2C = "use this list's END entry" (a match ends the walk, nothing is queued).
    #[serde(default)]
    pub transition: u32,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Shared {
    pub shared_cancels: SharedCancels,
    pub sequences: Sequences,
    #[serde(default)]
    pub reversal_families: ReversalFamilies,
}

/// Reversal families (cond 0x4B): lists of motion tags.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct ReversalFamilies {
    pub families: Vec<Vec<i32>>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct SharedCancels {
    pub entries: Vec<Cancel>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Sequences {
    pub entries: Vec<Sequence>,
}

/// A motion input (e.g. qcf+1+2): steps matched backwards through the input history within `window` frames.
#[derive(Clone, Debug, Deserialize)]
pub struct Sequence {
    pub cmd: u32,
    pub window: u32,
    pub steps: Vec<Step>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Step {
    /// The step word (dir | buttons << 8 | any-of << 12): the CPU AI plays it as one frame of input.
    #[serde(default)]
    pub raw: u32,
    pub dir: u32,
    pub buttons: Vec<u32>,
    #[serde(default)]
    pub any_of: bool,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Reactions {
    pub entries: Vec<ReactionEntry>,
    pub pushback_curves: PushbackCurves,
    pub air_reactions: AirReactions,
    /// Fixed pushback of air (juggle) hits (0x80019D2C), per frame.
    pub air_pushback_curve: AirPushback,
    pub throw_victims: ThrowVictims,
    pub close_range: CloseRange,
}

#[derive(Clone, Debug, Deserialize)]
pub struct AirPushback {
    pub samples: Vec<i32>,
}

/// Common air (juggle) reactions (0x8001A630): target height and alias.
#[derive(Clone, Debug, Deserialize)]
pub struct AirReactions {
    pub entries: Vec<AirReaction>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct AirReaction {
    pub target_height: i32,
    pub alias: usize,
}

/// Victim aliases of throws and holds (0x800174C4), by reaction id & 0xFFF.
#[derive(Clone, Debug, Deserialize)]
pub struct ThrowVictims {
    pub entries: Vec<usize>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct CloseRange {
    pub entries: Vec<CloseRangeEntry>,
}

/// Close-range reaction (0x800177DC): used when the pair is nearer than `max_distance`.
#[derive(Clone, Debug, Deserialize)]
pub struct CloseRangeEntry {
    pub reaction_id: usize,
    pub max_distance: i32,
}

#[derive(Clone, Debug, Deserialize)]
pub struct PushbackCurves {
    pub entries: Vec<PushbackCurve>,
}

/// Pushback after a hit or block (table 0x8001075C): `constant_frames` frames at `velocity`, plus an impulse on
/// each of the first 8 frames; units per frame along the pushback direction.
#[derive(Clone, Debug, Deserialize)]
pub struct PushbackCurve {
    /// Reference value used by the reaction parameters (a multiple of 10).
    pub index: u32,
    pub constant_frames: i32,
    pub velocity: i32,
    pub impulses: [i32; 8],
}

#[derive(Clone, Debug, Deserialize)]
pub struct ReactionEntry {
    /// 14 victim move aliases (slot order: kumite_converter's reference engine.md §7.5).
    pub victims: Vec<usize>,
    #[serde(default)]
    pub params: Vec<i32>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct MovesetIndex {
    pub models: Vec<ModelEntry>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ModelEntry {
    pub model_record: usize,
    pub glb: String,
    pub moveset_file: String,
    pub moveset_id: i64,
    /// Tekken Tag: its hands (the game layer's `hands`).
    #[serde(default)]
    pub hands: Option<HandsEntry>,
    /// Tekken Tag: its scale of the motions' root (player +0x4FC, 4096 = 1); None: 4096.
    #[serde(default)]
    pub motion_scale: Option<i32>,
}

/// A Tekken Tag model's hands: `file` its poses (characters/), `default` the kind the load eases both hands to and
/// op kind 9 means (0x416BA0), `ops` whether the hand ops move them (True Ogre's move his wings instead).
#[derive(Clone, Debug, Deserialize)]
pub struct HandsEntry {
    pub file: String,
    pub default: u32,
    pub ops: bool,
}
