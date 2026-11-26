//! Tekken 3's floor colours: each frame the PS1 works out a colour (128 = as drawn) for each vertex of the 11 x 11
//! grid it lays round the fighters' midpoint, and its POLY_GT4s shade between them (0x80048E58, 0x80049848).

use kumite_assets::FloorShading;

/// The grid's middle (x, z in the PS1's axes, game units) and its vertices' colours, a row of 11 (x from the least) a
/// z from the least. `roots`: the fighters' roots (P+0xF68; x, z), `eye` the eye's (x, z), `depth` a floor point's SZ.
pub fn shades(l: &FloorShading, roots: [[i32; 2]; 2], eye: [i32; 2], depth: impl Fn(i32, i32) -> i32, true_ogre: bool, decided: bool) -> ([i32; 2], [u8; 121]) {
    let tile = l.tile;
    let mid = [0, 1].map(|k| (roots[0][k] + roots[1][k]) / 2);
    // the middle: the midpoint snapped to whole tiles, one less below 0 (0x80049988)
    let centre = mid.map(|m| {
        let a = m + tile / 2;
        (a / tile - i32::from(a < 0)) * tile
    });
    // True Ogre in the fight: mode 0 lit as mode 2 (0x8004870C)
    let (mode, cap) = if l.mode == 0 && true_ogre { (2, l.true_ogre_cap) } else { (l.mode, l.cap) };
    // mode 0: the eye's distance to the midpoint (SquareRoot0) and the fade's step (0x80049290, 0x80049588)
    let near = kumite_sim::gte_sqrt((eye[0] - mid[0]).saturating_pow(2).saturating_add((eye[1] - mid[1]).saturating_pow(2)));
    let step = 0x400_0000 / l.fade_depth[usize::from(decided)];
    let over = |k: [i32; 2], x: i32, z: i32| (i64::from(k[0] - x).pow(2) + i64::from(k[1] - z).pow(2)).max(1);
    let mut out = [128u8; 121];
    for (n, c) in out.iter_mut().enumerate() {
        let (x, z) = (centre[0] + (n as i32 % 11 - 5) * tile, centre[1] + (n as i32 / 11 - 5) * tile);
        let v = match mode {
            1 => (roots.iter().map(|r| i64::from(l.spot[0]) / over(*r, x, z)).sum::<i64>()).min(254),
            2 => (i64::from(l.spot[1]) / over(mid, x, z)).min(i64::from(cap)),
            0 => {
                let k = (i64::from((depth(x, z).clamp(0, 0xFFFF) - near).max(0)) * i64::from(step)) >> 16;
                let t = l.fade.get(k.min(1023) as usize).copied().unwrap_or(0);
                (128 + ((i64::from(t) * i64::from(l.shade)) >> 7)).clamp(0, 254)
            }
            _ => 128,
        };
        *c = v.clamp(0, 254) as u8;
    }
    (centre, out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn light(mode: u32) -> FloorShading {
        FloorShading { mode, shade: -51, tile: 1800, spot: [0x0F20_0000, 0x2FAF_0800], cap: 192, true_ogre_cap: 192, fade: vec![0; 1024], fade_depth: [10000; 2] }
    }

    /// The PS1's values (0x8004A12C in PCSX-Redux): VS on stages 9 (mode 2) and 6 (mode 1), roots (-62, 55) and
    /// (833, 1); (x, z, colour).
    #[test]
    fn spotlights_as_the_ps1() {
        let roots = [[-62, 55], [833, 1]];
        for (mode, want) in
            [(2, [(-1800, 0, 167), (0, 0, 192), (-1800, -1800, 98), (1800, -1800, 149), (3600, 0, 77)]), (1, [(-1800, 0, 119), (0, 0, 254), (-1800, -1800, 63), (3600, -1800, 38), (-3600, 1800, 27)])]
        {
            let (centre, s) = shades(&light(mode), roots, [214, -5487], |_, _| 0, false, false);
            assert_eq!(centre, [0, 0]);
            for (x, z, c) in want {
                assert_eq!(s[((z / 1800 + 5) * 11 + x / 1800 + 5) as usize], c, "mode {mode} at {x}, {z}");
            }
        }
    }

    #[test]
    fn grid_snaps_as_the_ps1() {
        let (centre, _) = shades(&light(2), [[-1000, 899], [-1000, 900]], [0, 0], |_, _| 0, false, false);
        assert_eq!(centre, [-1800, 0]);
        // (an exact negative multiple goes a tile further, as the PS1's)
        let (centre, _) = shades(&light(2), [[-2700, 0], [-2700, 0]], [0, 0], |_, _| 0, false, false);
        assert_eq!(centre, [-3600, 0]);
    }
}
