//! Pad input → move notation, the 60-frame history, and command matching.
//!
//! Directions are numpad digits relative to the fighter's facing (6 = towards the opponent, 4 = away,
//! 2 = down, 8 = up; 5 = neutral). Buttons: 1 = left punch, 2 = right punch, 3 = left kick, 4 = right kick.
//! Each frame the history stores `new_buttons << 4 | dir` (EXE 0x8002C580).

use crate::data::{Sequence, Step};

/// Raw controller state for one frame (screen-relative directions).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Pad {
    pub up: bool,
    pub down: bool,
    pub left: bool,
    pub right: bool,
    /// Buttons 1..4 as bits 0..3; Tekken Tag's tag button as bit 4 ([`TAG`]).
    pub buttons: u8,
    /// START, pressed this frame (online only, where it goes through the rollback's pads: it skips a replay or a
    /// win pose on every computer at once; the fight itself never reads it).
    pub start: bool,
}

pub const HISTORY: usize = 60;

/// The tag button's bit in [`Pad::buttons`]; read only in a tag battle (Tekken Tag), where its commands carry
/// [`TAG_COMMAND`].
pub const TAG: u8 = 0x10;
/// A plain command's tag-button bit (Tekken Tag's command bit 14; Tekken 3's commands never have it).
pub const TAG_COMMAND: u32 = 0x4000;

#[derive(Clone, Debug)]
pub struct InputState {
    /// Current numpad direction (1..9).
    pub dir: u32,
    /// Buttons held / newly pressed this frame (bits 0..3 = 1..4).
    pub held: u32,
    pub pressed: u32,
    /// The tag button held / newly pressed this frame (kept apart: the four buttons' tests never see it).
    pub tag_held: bool,
    pub tag: bool,
    /// Ring of `pressed << 4 | dir`, newest at `head`.
    history: [u8; HISTORY],
    head: usize,
}

impl Default for InputState {
    fn default() -> Self {
        Self { dir: 5, held: 0, pressed: 0, tag_held: false, tag: false, history: [5; HISTORY], head: 0 }
    }
}

impl InputState {
    /// Advance one frame. `facing_right`: the opponent is to the right on screen (P1 side).
    pub fn update(&mut self, pad: Pad, facing_right: bool) {
        let (fwd, back) = if facing_right { (pad.right, pad.left) } else { (pad.left, pad.right) };
        let h = if fwd {
            1
        } else if back {
            -1
        } else {
            0
        };
        let v = if pad.up {
            1
        } else if pad.down {
            -1
        } else {
            0
        };
        self.dir = (5 + h + 3 * v) as u32;
        let held = pad.buttons as u32 & 0xF;
        self.pressed = held & !self.held;
        self.held = held;
        let tag = pad.buttons & TAG != 0;
        (self.tag, self.tag_held) = (tag && !self.tag_held, tag);
        self.head = (self.head + 1) % HISTORY;
        self.history[self.head] = (self.pressed << 4 | self.dir) as u8;
    }

    /// History entry `ago` frames back (0 = this frame): (new buttons, direction).
    pub fn past(&self, ago: usize) -> (u32, u32) {
        let e = self.history[(self.head + HISTORY - ago % HISTORY) % HISTORY] as u32;
        (e >> 4, e & 15)
    }

    /// Single-frame command (EXE 0x8002CC28). `cmd` ≤ 0xBFFF:
    /// bits 5..13 = accepted directions (bit 4+n for numpad n), bits 0..3 = buttons, bit 4 = "any of", bit 14 = the
    /// tag button, newly pressed (Tekken Tag).
    pub fn matches_command(&self, cmd: u32) -> bool {
        let dirs = cmd & 0x3FE0;
        if dirs != 0 && cmd & (1 << (self.dir + 4)) == 0 {
            return false;
        }
        let buttons = cmd & 0xF;
        if cmd & TAG_COMMAND != 0 {
            // shortcut: tag + a button takes the button held, as the four buttons' "all held" test would
            return self.tag && self.held & buttons == buttons;
        }
        if cmd & 0x1F == 0x10 {
            return true; // direction held, no button (walk, crouch, jump)
        }
        if cmd & 0x10 != 0 {
            return buttons & self.pressed != 0; // any one of the listed buttons, newly pressed
        }
        if buttons == 0 {
            return self.pressed == 0; // a pure direction: no new button this frame
        }
        // every listed button held, at least one of them new this frame
        buttons & self.pressed != 0 && self.held & buttons == buttons
    }

    /// Motion input (EXE 0x8002CA84): the last step must happen now; earlier steps are found going back
    /// through the history, all within `window` frames. Frames without a new button may sit between steps.
    pub fn matches_sequence(&self, seq: &Sequence) -> bool {
        let Some((last, rest)) = seq.steps.split_last() else { return false };
        if !self.step_now(last, 0) {
            return false;
        }
        let mut ago = 1;
        for step in rest.iter().rev() {
            loop {
                if ago as u32 > seq.window || ago >= HISTORY {
                    return false;
                }
                let found = self.step_now(step, ago);
                ago += 1;
                if found {
                    break;
                }
            }
        }
        true
    }

    fn step_now(&self, step: &Step, ago: usize) -> bool {
        let (new, dir) = self.past(ago);
        if step.dir != 0 && step.dir != dir {
            return false;
        }
        let want: u32 = step.buttons.iter().filter(|b| (1..=4).contains(*b)).map(|b| 1 << (b - 1)).sum();
        if want == 0 {
            return true;
        }
        if step.any_of {
            new & want != 0
        } else {
            new & want == want
        }
    }

    /// Dash (f, n, f) or backdash (b, n, b) within a short window (EXE 0x8002C8B4 / 0x8002C99C; the exact
    /// window is not yet traced — 12 frames is a placeholder).
    pub fn matches_dash(&self, forward: bool) -> bool {
        let d = if forward { 6 } else { 4 };
        if self.dir != d || self.past(1).1 == d {
            return false; // the second tap must start this frame
        }
        let mut ago = 1;
        while ago < 12 && self.past(ago).1 != d {
            if self.past(ago).1 != 5 {
                return false;
            }
            ago += 1;
        }
        ago < 12 && self.past(ago).1 == d
    }

    /// Tekken Tag's dash / backdash (0x36CAF8, window 20): the second tap starts this frame after neutral, and the
    /// gap and the first tap, all but its first frame, last under 20 frames together (a long walk then a tap is not
    /// a dash).
    pub fn matches_tag_dash(&self, forward: bool) -> bool {
        let d = if forward { 6 } else { 4 };
        if self.dir != d || self.past(1).1 != 5 {
            return false;
        }
        let (mut left, mut ago) = (19, 2);
        // the gap, then the first tap: each frame but the tap's newest uses one of the window
        let mut tap = false;
        while left > 0 {
            let v = self.past(ago).1;
            ago += 1;
            if !tap && v == d {
                tap = true;
            } else if v == if tap { d } else { 5 } {
                left -= 1;
            } else {
                return tap;
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed(s: &mut InputState, pads: &[Pad]) {
        for p in pads {
            s.update(*p, true);
        }
    }

    #[test]
    fn single_frame_commands() {
        let mut s = InputState::default();
        feed(&mut s, &[Pad { down: true, right: true, buttons: 0b0001, ..Default::default() }]);
        assert_eq!(s.dir, 3);
        assert!(s.matches_command(0x0081)); // d/f+1: bit 4+3 = 0x80, button 1
        assert!(!s.matches_command(0x0083)); // d/f+1+2 needs 2 held too
        feed(&mut s, &[Pad { down: true, right: true, buttons: 0b0001, ..Default::default() }]);
        assert!(!s.matches_command(0x0081)); // 1 is held, not new
                                             // the tag button: only its own commands see it, newly pressed
        feed(&mut s, &[Pad { buttons: TAG, ..Default::default() }]);
        assert!(s.matches_command(TAG_COMMAND) && s.matches_command(0x0010) && !s.matches_command(0x0001));
        feed(&mut s, &[Pad { buttons: TAG | 2, ..Default::default() }]);
        assert!(!s.matches_command(TAG_COMMAND) && s.pressed == 2);
    }

    /// Tekken Tag's dash: a tap, a gap, a tap within 20 frames; a long walk then a tap is not one (PCSX2).
    #[test]
    fn tag_dash_takes_a_tap_not_a_walk() {
        let dash = |first: usize, gap: usize| {
            let mut s = InputState::default();
            let (f, n) = (Pad { right: true, ..Default::default() }, Pad::default());
            feed(&mut s, &[n; 3]);
            feed(&mut s, &vec![f; first]);
            feed(&mut s, &vec![n; gap]);
            feed(&mut s, &[f]);
            s.matches_tag_dash(true)
        };
        assert!(dash(2, 3) && dash(1, 19) && dash(11, 9));
        assert!(!dash(40, 4) && !dash(1, 20) && !dash(12, 9));
    }

    #[test]
    fn quarter_circle_forward() {
        let seq = Sequence {
            cmd: 0xC011,
            window: 25,
            steps: vec![
                Step { raw: 0, dir: 5, buttons: vec![], any_of: false },
                Step { raw: 0, dir: 2, buttons: vec![], any_of: false },
                Step { raw: 0, dir: 3, buttons: vec![], any_of: false },
                Step { raw: 0, dir: 6, buttons: vec![1, 2], any_of: false },
            ],
        };
        let mut s = InputState::default();
        let p = |d: bool, f: bool, b: u8| Pad { down: d, right: f, buttons: b, ..Default::default() };
        feed(&mut s, &[p(false, false, 0), p(true, false, 0), p(true, true, 0), p(false, true, 0b11)]);
        assert!(s.matches_sequence(&seq));
    }
}
