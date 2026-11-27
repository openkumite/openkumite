//! Tekken 3's floor colours: each frame the PS1 works out a colour (128 = as drawn) for each vertex of the 11 x 11
//! grid it lays round the fighters' midpoint, and its POLY_GT4s shade between them (0x80048E58, 0x80049848).

use kumite_assets::FloorShading;

/// The grid's middle (x, z in the PS1's axes, game units) and its vertices' colours, a row of 11 (x from the least) a
/// z from the least. `roots`: the fighters' roots (P+0xF68; x, z; Tekken Force: the player's first), `eye` the eye's
/// (x, z), `depth` a floor point's SZ; `force`: in Tekken Force (battle type 8), on its final stage (0x800AFAAC = 4).
pub fn shades(l: &FloorShading, roots: [[i32; 2]; 2], eye: [i32; 2], depth: impl Fn(i32, i32) -> i32, true_ogre: bool, decided: bool, force: Option<bool>) -> ([i32; 2], [u8; 121]) {
    let tile = l.tile;
    let mid = [0, 1].map(|k| (roots[0][k] + roots[1][k]) / 2);
    // True Ogre in the fight: mode 0 lit as mode 2; Force's final stage: mode 4 (0x8004870C)
    let (mode, cap) = match force {
        Some(true) => (4, 254),
        None if l.mode == 0 && true_ogre => (2, l.true_ogre_cap),
        _ => (l.mode, l.cap),
    };
    // the middle: the midpoint (Force: the player's root half a tile on, 0x80049928; the fade's x at z 325,
    // 0x80048FDC) snapped to whole tiles, one less below 0 (0x80049988)
    let at = match force {
        Some(_) if mode != 0 => [roots[0][0], roots[0][1] + tile / 2],
        Some(_) => [mid[0], 325],
        None => mid,
    };
    let centre = at.map(|m| {
        let a = m + tile / 2;
        (a / tile - i32::from(a < 0)) * tile
    });
    // mode 0: the eye's distance to the midpoint (SquareRoot0; Force: the eye's z) and the fade's step (0x80049290,
    // 0x800492A0, 0x80049588)
    let near = match force {
        Some(_) => eye[1].saturating_abs(),
        None => kumite_sim::gte_sqrt((eye[0] - mid[0]).saturating_pow(2).saturating_add((eye[1] - mid[1]).saturating_pow(2))),
    };
    let step = 0x400_0000 / l.fade_depth[usize::from(decided)];
    let over = |k: [i32; 2], x: i32, z: i32| (i64::from(k[0] - x).pow(2) + i64::from(k[1] - z).pow(2)).max(1);
    let spot = |k: usize| i64::from(l.spot.get(k).copied().unwrap_or(0));
    let mut out = [128u8; 121];
    for (n, c) in out.iter_mut().enumerate() {
        let (x, z) = (centre[0] + (n as i32 % 11 - 5) * tile, centre[1] + (n as i32 / 11 - 5) * tile);
        let v = match mode {
            1 => (roots.iter().map(|r| spot(0) / over(*r, x, z)).sum::<i64>()).min(254),
            2 => (spot(1) / over(mid, x, z)).min(i64::from(cap)),
            // Force: 3 on the player (0x800B297C), 4 on the eye's x at z -2000 (0x8004A0A4)
            3 => (spot(2) / over(roots[0], x, z)).min(254),
            4 => (spot(3) / over([eye[0], -2000], x, z)).min(254),
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
        FloorShading { mode, shade: -51, tile: 1800, spot: vec![0x0F20_0000, 0x2FAF_0800, 0x1E84_8000, 0x18B8_2000], cap: 192, true_ogre_cap: 192, fade: vec![0; 1024], fade_depth: [10000; 2] }
    }

    /// The PS1's values (0x8004A12C in PCSX-Redux): VS on stages 9 (mode 2) and 6 (mode 1), roots (-62, 55) and
    /// (833, 1); (x, z, colour).
    #[test]
    fn spotlights_as_the_ps1() {
        let roots = [[-62, 55], [833, 1]];
        for (mode, want) in
            [(2, [(-1800, 0, 167), (0, 0, 192), (-1800, -1800, 98), (1800, -1800, 149), (3600, 0, 77)]), (1, [(-1800, 0, 119), (0, 0, 254), (-1800, -1800, 63), (3600, -1800, 38), (-3600, 1800, 27)])]
        {
            let (centre, s) = shades(&light(mode), roots, [214, -5487], |_, _| 0, false, false, None);
            assert_eq!(centre, [0, 0]);
            for (x, z, c) in want {
                assert_eq!(s[((z / 1800 + 5) * 11 + x / 1800 + 5) as usize], c, "mode {mode} at {x}, {z}");
            }
        }
    }

    /// The PS1's values in Tekken Force (0x8004A12C): stage 3 (mode 3) with the player's root at (8, -10), the final
    /// stage (mode 4) with it at (-735, -2200) and the eye's x -1435; (x, z, colour).
    #[test]
    fn force_spotlights_as_the_ps1() {
        let force = |mode| FloorShading { tile: 2500, ..light(mode) };
        for (l, roots, eye, last, want) in [
            (
                force(3),
                [[8, -10], [4200, 1100]],
                [0, -7000],
                false,
                vec![(-5000, 0, 20), (-2500, 0, 81), (-5000, -2500, 16), (-2500, -2500, 40), (0, 0, 254), (0, -2500, 82), (2500, 0, 82), (2500, -2500, 41), (5000, 2500, 16)],
            ),
            (
                force(0),
                [[-735, -2200], [916, -1051]],
                [-1435, -7000],
                true,
                vec![
                    (-5000, -2500, 32),
                    (-2500, -2500, 254),
                    (-5000, -5000, 19),
                    (-2500, -5000, 40),
                    (0, -2500, 179),
                    (0, -5000, 37),
                    (2500, -5000, 16),
                    (-2500, 0, 80),
                    (0, 0, 68),
                    (-7500, 2500, 7),
                    (-7500, 0, 10),
                    (2500, 2500, 11),
                ],
            ),
        ] {
            let (centre, s) = shades(&l, roots, eye, |_, _| 0, false, false, Some(last));
            assert_eq!(centre, [0, 0]);
            for (x, z, c) in want {
                assert_eq!(s[((z / 2500 + 5) * 11 + x / 2500 + 5) as usize], c, "mode {} at {x}, {z}", l.mode);
            }
        }
    }

    #[test]
    fn grid_snaps_as_the_ps1() {
        let (centre, _) = shades(&light(2), [[-1000, 899], [-1000, 900]], [0, 0], |_, _| 0, false, false, None);
        assert_eq!(centre, [-1800, 0]);
        // (an exact negative multiple goes a tile further, as the PS1's)
        let (centre, _) = shades(&light(2), [[-2700, 0], [-2700, 0]], [0, 0], |_, _| 0, false, false, None);
        assert_eq!(centre, [-3600, 0]);
    }
}
