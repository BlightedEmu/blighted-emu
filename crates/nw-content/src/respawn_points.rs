// SPDX-License-Identifier: AGPL-3.0-or-later
//! The open world's respawn points: where the server puts a dead player who asked to respawn at a
//! point (1938, `[<id>]<digit>`). Retail never respawns a player in place: the client picks a
//! respawn point it knows (a settlement, an inn, a camp, a house, a public point) and the server
//! moves the player there (respawn note, 2026-09-18: three deaths moved 305, 406 and 541 m; two
//! with one id landed 3.9 m apart).
//!
//! The points are Observed: every 1938 of the retail captures made in the open world (97 of 317
//! requests; the others were in dungeons or before their capture saw the level), with the
//! player's median position 1.5 s after it. Their entities' own positions are client level data
//! this server does not read yet (Unknown), so a point no capture shows falls back to the known
//! point nearest the death (Inferred).

/// Each Observed respawn point: its id and where retail put the player, metres.
const OPEN_WORLD_RESPAWN_POINTS: [(u64, [f32; 3]); 38] = [
    (790_201_786_363_326_443, [4212.0, 6626.7, 278.3]),
    (959_623_501_612_939_235, [8619.5, 2580.2, 73.0]),
    (1_175_320_635_480_433_299, [4779.3, 6799.1, 159.6]),
    (1_343_403_275_282_712_200, [1306.8, 11277.8, 65.9]),
    (1_383_406_891_496_616_603, [10890.0, 3311.9, 77.7]),
    (1_744_065_511_813_566_098, [3541.4, 4847.3, 236.5]),
    (2_283_807_676_330_519_411, [2387.1, 4334.8, 40.4]),
    (2_738_630_951_191_017_995, [6038.3, 5278.9, 56.8]),
    (2_845_219_926_347_408_467, [11897.4, 7692.1, 220.9]),
    (3_643_404_068_721_014_412, [3541.4, 4847.3, 236.4]),
    (3_699_928_999_510_224_819, [9263.6, 4746.2, 136.8]),
    (3_924_519_360_713_777_550, [1313.8, 11284.8, 66.9]),
    (4_067_284_797_430_401_254, [4275.2, 6046.6, 64.4]),
    (4_195_408_266_106_496_367, [3987.9, 5875.4, 140.2]),
    (4_541_370_231_610_797_003, [10030.7, 3194.7, 173.9]),
    (5_136_119_961_217_234_610, [6943.7, 1278.2, 62.6]),
    (5_358_205_357_797_302_819, [7239.7, 375.4, 45.3]),
    (5_711_626_716_046_173_619, [7243.7, 5439.6, 115.0]),
    (8_961_798_132_649_693_091, [2343.8, 4295.1, 45.3]),
    (9_086_712_569_067_387_272, [6625.9, 1227.5, 45.7]),
    (9_518_795_765_143_207_835, [7074.2, 665.2, 92.3]),
    (9_995_419_549_178_159_732, [4935.1, 7790.4, 225.6]),
    (10_394_395_908_515_210_563, [6972.5, 1204.4, 67.2]),
    (10_551_023_067_555_017_587, [9888.0, 5181.2, 170.9]),
    (10_773_042_253_153_734_859, [10980.0, 2341.2, 53.9]),
    (11_664_752_315_738_743_067, [7534.1, 3987.9, 120.0]),
    (12_663_701_595_423_529_582, [4522.5, 7246.2, 159.4]),
    (14_384_443_528_883_955_635, [13702.4, 6284.8, 54.2]),
    (14_394_457_913_679_277_227, [9178.5, 450.9, 71.5]),
    (14_639_843_741_699_618_923, [6865.4, 841.8, 51.4]),
    (14_836_115_501_405_101_091, [6617.8, 1198.2, 43.8]),
    (15_156_449_890_035_693_675, [7131.4, 2393.4, 82.3]),
    (15_384_995_778_846_961_608, [6851.7, 1231.6, 62.0]),
    (15_965_492_676_612_408_915, [2608.9, 4554.7, 60.4]),
    (16_348_430_920_758_652_246, [5986.6, 5364.7, 58.0]),
    (16_695_474_225_659_806_323, [2399.5, 4248.0, 41.8]),
    (17_175_753_972_668_589_968, [2389.3, 4770.2, 188.3]),
    (17_808_084_344_869_652_339, [8745.4, 9900.5, 547.3]),
];

/// Where the open-world respawn point `id` puts a player, when a retail capture showed it.
pub fn respawn_point(id: u64) -> Option<[f32; 3]> {
    OPEN_WORLD_RESPAWN_POINTS
        .iter()
        .find(|(point, _)| *point == id)
        .map(|(_, at)| *at)
}

/// The known open-world respawn point nearest `at` (horizontally).
pub fn nearest_respawn_point(at: [f32; 3]) -> [f32; 3] {
    let distance = |point: &[f32; 3]| (point[0] - at[0]).hypot(point[1] - at[1]);
    OPEN_WORLD_RESPAWN_POINTS
        .iter()
        .map(|(_, point)| *point)
        .min_by(|a, b| distance(a).total_cmp(&distance(b)))
        .unwrap_or(at)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_retail_respawn_point_is_where_retail_put_the_player() {
        // FFA_LightningStrikeDeath and FFA_BallLightningDeath: (6864.1, 840.3) and (6866.7, 843.3).
        let at = respawn_point(14_639_843_741_699_618_923).expect("known point");
        assert!(
            (at[0] - 6865.4).abs() < 0.1 && (at[1] - 841.8).abs() < 0.1,
            "{at:?}"
        );
        assert_eq!(respawn_point(1), None);
    }

    #[test]
    fn an_unknown_point_falls_back_to_the_nearest_known_one() {
        // Near the start beach (7300, 3090): the point at (7131.4, 2393.4), 717 m away.
        let nearest = nearest_respawn_point([7300.0, 3090.0, 45.0]);
        assert_eq!(nearest, [7131.4, 2393.4, 82.3]);
    }
}
