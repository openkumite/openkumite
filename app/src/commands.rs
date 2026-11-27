//! Tekken 3's command list (0x80078550): a fighter's moves, its name over its command, in a column down one half
//! of the screen. The pause's COMMAND (0x80078DF8) opens it for every human, Practice's COMMAND LIST (0x80078BCC)
//! for the player. screens.json `hud.commands`: the lists by model record, the places and colours; the printer
//! (0x80077724) draws the text and the moves' words from the fighter's sheet (pictures/names/model_NNN_commands_index
//! .png, 1-bit planes) and the arrows and buttons from record 10's pad pictures.

use crate::art::{Art, Screen as View, Tex};
use egui::Color32;
use serde_json::Value;

/// One side's place in its list: the move at the top's row and how far (pixels) the list still eases to it.
#[derive(Clone, Copy, Default)]
pub struct Scroll {
    pub index: i32,
    pub offset: i32,
    /// The direction held and the frames since its press (the pad's auto-repeat, 0x80029F58).
    held: (Option<crate::menu::Key>, u32),
}

fn num(v: &Value) -> i32 {
    v.as_i64().unwrap_or(0) as i32
}

fn rgb(v: &Value) -> Color32 {
    Color32::from_rgb(num(&v[0]) as u8, num(&v[1]) as u8, num(&v[2]) as u8)
}

/// Model `record`'s moves (name, command bytes).
fn list(d: &Value, record: usize) -> &[Value] {
    d["lists"][record.to_string()].as_array().map_or(&[], Vec::as_slice)
}

fn bytes(v: &Value) -> Vec<u8> {
    v.as_array().into_iter().flatten().map(|b| b.as_u64().unwrap_or(0) as u8).collect()
}

impl Scroll {
    /// A direction of the side's pad (0x80078550): up / down one move, left / right five, round the list; the list
    /// eases a row a move. True when it moved (its sound).
    pub fn key(&mut self, d: &Value, record: usize, key: crate::menu::Key) -> bool {
        use crate::menu::Key;
        let n = list(d, record).len() as i32;
        let [one, five] = [num(&d["rows"]["scroll"]["step"][0]), num(&d["rows"]["scroll"]["step"][1])];
        let step = match key {
            Key::Down => one,
            Key::Up => -one,
            Key::Right => five,
            Key::Left => -five,
            _ => return false,
        };
        if n == 0 {
            return false;
        }
        let mut i = if self.index < n { self.index } else { 0 } + step;
        if i < 0 {
            i += n;
        }
        if i >= n {
            i -= n;
        }
        self.index = i;
        self.offset += step * num(&d["rows"]["pitch"]);
        true
    }

    /// A frame: the direction held repeats as the pad's auto-repeat (`repeat`: the frames to the first repeat, then
    /// between them), and the list eases a third of the way (rounded up) to its place. True when it moved.
    pub fn step(&mut self, d: &Value, record: usize, dir: Option<crate::menu::Key>, repeat: [u32; 2]) -> bool {
        let (was, n) = self.held;
        self.held = if dir == was { (dir, n + 1) } else { (dir, 0) };
        let n = self.held.1;
        let [first, every] = repeat.map(|r| r.max(1));
        let moved = dir.filter(|_| n >= first && (n - first).is_multiple_of(every)).is_some_and(|k| self.key(d, record, k));
        let third = num(&d["rows"]["scroll"]["ease"]).max(1);
        let a = self.offset.abs();
        self.offset = self.offset.signum() * (a - (a + third) / third).max(0);
        moved
    }
}

/// The text's length in letters as 0x80079208 counts it: a new line 100, 0xFB 10, a code from 0xA1 2, a word its
/// width, a letter 1.
fn length(text: &[u8], widths: &Value) -> i32 {
    text.iter()
        .map(|&c| match c {
            0xFC => 100,
            0xFB => 10,
            0xA1.. => 2,
            1..=0x1F => num(&widths[c as usize - 1]),
            _ => 1,
        })
        .sum()
}

/// Where 0x80079170 ends a long name's first line (`budget` letters; a word adds its width, as the PS1 does) and
/// where its rest starts.
fn split(text: &[u8], mut budget: i32, widths: &Value) -> (usize, usize) {
    for (k, &c) in text.iter().enumerate() {
        match c {
            0xFC => return (k, k + 1),
            0xFB => budget -= 10,
            0xA1.. => budget -= 2,
            1..=0x1F => budget += num(&widths[c as usize - 1]),
            _ => budget -= 1,
        }
        if budget <= 0 {
            return (k + 1, k + 1);
        }
    }
    (text.len(), text.len())
}

/// The printer (0x80077724) at (x, y): letters and words from the sheet's planes, the pad's arrows and buttons.
#[allow(clippy::too_many_arguments)]
fn print(painter: &egui::Painter, v: &View, art: &Art, d: &Value, sheet: &str, text: &[u8], x: f32, y: f32, tint: Color32) {
    let s = &d["sheet"];
    let (adv, line) = (num(&s["advance"]) as f32, num(&s["line"]) as f32);
    let widths = &s["word_widths"];
    let plane_clut = |p: i32| [(num(&s["plane_clut"][0]) + 16 * p) as u32, num(&s["plane_clut"][1]) as u32];
    let icons = &d["icons"];
    let (iw, ih, idy, iadv) = (num(&icons["w"]) as u32, num(&icons["h"]) as u32, num(&icons["dy"]) as f32, num(&icons["advance"]) as f32);
    let icon = |u: i32, clut: [u32; 2]| Tex { page: [num(&icons["page"][0]) as u32, num(&icons["page"][1]) as u32], bpp: 4, u: u as u32, v: num(&icons["v"]) as u32, clut: Some(clut) };
    let pair = |c: &Value| [num(&c[0]) as u32, num(&c[1]) as u32];
    let (x0, mut x, mut y) = (x, x, y);
    // a piece of the sheet: (sheet x, row, plane, letters wide)
    let piece = |at: (i32, i32, i32, f32), x: f32, y: f32| {
        let (sx, row, plane, w) = at;
        art.indexed_part(painter, v, &["exe"], sheet, plane_clut(plane), [sx as f32, (row as f32) * line, w, line], [x, y, w, line], tint);
    };
    for &c in text {
        match c {
            0x80..=0x90 => {
                // an arrow (0x800775C0): its picture and flips; 0x88-0x8F the tapped colour
                let i = c - 0x80;
                let a = &icons["arrows"][if i == 0x10 { 8 } else { usize::from(i & 7) }];
                let clut = pair(&icons[if i & 8 != 0 { "tapped_clut" } else { "held_clut" }]);
                let flip = num(&a["flip"]);
                let (w, h) = (iw as f32, ih as f32);
                let (dx, dw) = if flip & 1 != 0 { (x, -w) } else { (x, w) };
                let (dy, dh) = if flip & 2 != 0 { (y + idy + h, -h) } else { (y + idy, h) };
                art.quad(painter, v, &["r010"], &icon(num(&a["u"]), clut), iw, ih, [dx, dy, dw, dh], [tint; 4]);
                x += iadv;
            }
            0x91..=0xA0 => {
                // buttons: the outline, over it the dots lit by the bits' CLUT (768 + 16 * (b & 7), 508 + b / 8)
                let b = u32::from(c - 0x91);
                let bc = pair(&icons["buttons"]["clut"]);
                let outline = icon(num(&icons["buttons"]["outline_u"]), pair(&icons["buttons"]["outline_clut"]));
                art.sprite(painter, v, &["r010"], &outline, iw, ih, x, y + idy, tint);
                art.sprite(painter, v, &["r010"], &icon(num(&icons["buttons"]["u"]), [bc[0] + 16 * (b & 7), bc[1] + b / 8]), iw, ih, x, y + idy, tint);
                x += iadv;
            }
            0x20 | 0xFE => x += adv,
            0xFD => x += 2.0 * adv,
            0xFB => x += 10.0 * adv,
            0xFC => {
                x = x0;
                y += line;
            }
            0x21..=0x7F => {
                let a = i32::from(c - 0x20);
                piece(((a & 15) * adv as i32, a >> 6, (a & 0x30) >> 4, adv), x, y);
                x += adv;
            }
            0 => break,
            _ => {
                // a word: codes 1-0x1F and 0xA1-0xA3 down the left column, 0xA4- the right (u 0x7C)
                let (k, row, plane, sx) = match c {
                    1..=0x1F => (usize::from(c - 1), (i32::from(c) + 5) >> 2, (i32::from(c) + 1) & 3, 0),
                    _ if c - 0x82 < 0x22 => (usize::from(c - 0x82), (i32::from(c) - 0x7C) >> 2, (i32::from(c) - 0x80) & 3, 0),
                    _ => (usize::from(c - 0x82), (i32::from(c) - 0x9C) >> 2, (i32::from(c) - 0xA4) & 3, num(&s["right_u"]) - num(&s["u"])),
                };
                let w = num(&widths[k]) as f32 * adv;
                piece((sx, row, plane, w), x, y);
                x += w;
            }
        }
    }
}

/// Side `side`'s list (model `record`, character id `character`) as 0x80078550 draws it, at its half of the screen.
#[allow(clippy::too_many_arguments)]
pub fn draw(painter: &egui::Painter, v: &View, art: &Art, scroll: Scroll, side: usize, record: usize, character: usize) {
    let d = &art.screens["hud"]["commands"];
    let moves = list(d, record);
    let n = moves.len() as i32;
    let px = num(&d["panel"]["x"][side.min(1)]) as f32;
    let shade = |r: &Value, dy: f32, c: [Color32; 4]| Art::shade(painter, v, [px + num(&r[0]) as f32, dy + num(&r[1]) as f32, num(&r[2]) as f32, num(&r[3]) as f32], c);
    let p = &d["panel"];
    shade(&p["rect"], 0.0, [rgb(&p["colour"]); 4]);
    for l in p["lines"].as_array().into_iter().flatten() {
        shade(l, 0.0, [rgb(&p["line_colour"]); 4]);
    }
    let sheet = format!("names/model_{record:03}_commands_index.png");
    // (the texels times the colour: 0x80 their own)
    let t = |k: usize| (num(&d["tint"][k]) * 2).min(255) as u8;
    let tint = Color32::from_rgb(t(0), t(1), t(2));
    let heading = &p["heading"];
    print(painter, v, art, d, &sheet, &[num(&heading["word"]) as u8], px + num(&heading["x"]) as f32, num(&heading["y"]) as f32, tint);
    if n == 0 {
        return;
    }
    let r = &d["rows"];
    let clip = painter.with_clip_rect(v.rect(0.0, num(&p["clip"][0]) as f32, crate::art::SCREEN_W, (num(&p["clip"][1]) - num(&p["clip"][0])) as f32).intersect(painter.clip_rect()));
    // the rows from the one the easing shows at the top
    let off = scroll.offset;
    let shown = if off > 0 { off / 64 + 1 } else { off / 64 };
    let mut k = (scroll.index - shown).rem_euclid(n);
    let single = character == num(&r["single"]["character"]) as usize;
    let (mut y, rows) = if single { (num(&r["single"]["y"]), 1) } else { (off + num(&r["y"]) - shown * num(&r["pitch"]), num(&r["count"])) };
    let widths = &d["sheet"]["word_widths"];
    let st = &r["stances"];
    for _ in 0..rows {
        let (name, command) = moves.get(k as usize).map(|m| (bytes(&m[0]), bytes(&m[1]))).unwrap_or_default();
        let yf = y as f32;
        // the box's colours: Hwoarang's stances' own, their first rows flat headings
        let (mut boxc, mut band, mut heading) = (rgb(&r["colours"]["box"]), rgb(&r["colours"]["band"]), false);
        if character == num(&st["character"]) as usize {
            for s in (0..2).rev() {
                let from = num(&st["from"][s]);
                if k >= from {
                    (boxc, band, heading) = (rgb(&st["box"][s]), rgb(&st["band"][s]), k == from);
                    if heading {
                        band = rgb(&st["heading"]);
                    }
                    break;
                }
            }
        }
        let rect = |e: &Value| [px + num(&e[0]) as f32, yf + num(&e[1]) as f32, num(&e[2]) as f32, num(&e[3]) as f32];
        if heading {
            let b = rect(&r["border"]["rect"]);
            Art::shade(&clip, v, [b[0], yf + num(&r["box"]["rect"][1]) as f32, b[2], num(&r["box"]["rect"][3]) as f32], [boxc; 4]);
            Art::shade(&clip, v, rect(&r["band"]["rect"]), [band; 4]);
        } else {
            Art::shade(&clip, v, rect(&r["border"]["rect"]), [rgb(&r["border"]["colour"]); 4]);
            Art::shade(&clip, v, rect(&r["box"]["rect"]), [boxc; 4]);
            // the band: the box's colour at the top to its own at the bottom
            Art::shade(&clip, v, rect(&r["band"]["rect"]), [boxc, boxc, band, band]);
        }
        let tx = px + num(&r["text_x"]) as f32;
        if length(&name, widths) >= num(&r["long"]) {
            let (end, rest) = split(&name, num(&r["first_part"]), widths);
            print(&clip, v, art, d, &sheet, &name[..end], tx, yf, tint);
            print(&clip, v, art, d, &sheet, &name[rest.min(name.len())..], tx, yf + num(&r["long_dy"]) as f32, tint);
        } else {
            print(&clip, v, art, d, &sheet, &name, tx, yf + num(&r["name_dy"]) as f32, tint);
        }
        print(&clip, v, art, d, &sheet, &command, tx, yf + num(&r["command_dy"]) as f32, tint);
        k = (k + 1) % n;
        y += num(&r["pitch"]);
    }
}

/// "PUSH BUTTON TO EXIT" under side `side`'s list (font 0, its words in turn white and yellow).
pub fn draw_footer(painter: &egui::Painter, v: &View, art: &Art, side: usize) {
    let f = &art.screens["hud"]["commands"]["panel"]["footer"];
    let font = num(&f["font"]) as usize;
    let mut x = (num(&f["x"]) + num(&art.screens["hud"]["commands"]["panel"]["x"][1]) * side.min(1) as i32) as f32;
    let parts: Vec<&str> = f["parts"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
    for (k, part) in parts.iter().enumerate() {
        art.text(painter, v, font, num(&f["colours"][k]) as u32, x, num(&f["y"]) as f32, part, Color32::WHITE);
        x += art.text_w(font, part);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::menu::Key;

    fn data() -> Value {
        serde_json::json!({"lists": {"71": [[[65], [128]], [[66], [129]], [[67], [130]]]},
                           "rows": {"pitch": 64, "scroll": {"step": [1, 5], "ease": 3}}})
    }

    /// Up / down one move round the list, a row of easing each; the easing loses a third (rounded up) a frame
    /// (0x80078550).
    #[test]
    fn the_list_steps_round_and_eases() {
        let d = data();
        let mut s = Scroll::default();
        assert!(s.key(&d, 71, Key::Up));
        assert_eq!((s.index, s.offset), (2, -64));
        assert!(s.key(&d, 71, Key::Right));
        // (round once, as the PS1: past the end stays past it until the next step)
        assert_eq!(s.index, 4);
        let mut t = Scroll { offset: 64, ..Scroll::default() };
        let offsets: Vec<i32> = (0..6)
            .map(|_| {
                t.step(&d, 71, None, [19, 4]);
                t.offset
            })
            .collect();
        assert_eq!(offsets, [42, 27, 17, 11, 7, 4]);
    }

    /// A held direction repeats 19 frames after the press, then every 4.
    #[test]
    fn a_held_direction_repeats() {
        let d = data();
        let mut s = Scroll::default();
        let at: Vec<usize> = (0..28).filter(|_| s.step(&d, 71, Some(Key::Down), [19, 4])).collect();
        assert_eq!(at, [19, 23, 27]);
    }

    /// A name's length counts a word by its width, a new line 100 (0x80079208); the first line of a long one stops
    /// at a new line or when the letters run out (0x80079170).
    #[test]
    fn long_names_split_as_the_ps1() {
        let widths = serde_json::json!([17, 13, 4]);
        assert_eq!(length(b"ab\x03", &widths), 6);
        assert_eq!(length(b"a\xfcb", &widths), 102);
        assert_eq!(split(b"Spiral Tap\xfc[x]", 28, &widths), (10, 11));
        assert_eq!(split(&[b'a'; 40], 28, &widths), (28, 28));
    }
}
