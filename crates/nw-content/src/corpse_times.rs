//! How long a dead creature's body stays in the world before it goes, by creature slice
//! (Observed: the median from a creature's death entry to its slot's last group, over the deaths
//! of 150 retail open-world and side-quest ledgers of `captures-index.txt` and the retail
//! new-character run `newchar-retail-2026-09-18`; every creature with at least 2 deaths). A creature whose corpse can be skinned is not here: it stays until it
//! is skinned ([`SKINNED_CORPSE_MS`]).

/// How long a skinned corpse stays after its gatherable controller went Depleted (Observed: 5,503
/// to 5,567 ms over the 7 skinned boars of the same ledgers, median 5,530).
pub const SKINNED_CORPSE_MS: u64 = 5_530;

/// How long the body of a creature not in [`CORPSE_MS`] stays (Observed: the median of the
/// table's creatures; most humanoids stay 6 s).
pub const DEFAULT_CORPSE_MS: u64 = 6_065;

/// Each measured creature slice (lower case, as the creature animation table names it) and how
/// long its body stays, ms; sorted by slice.
pub const CORPSE_MS: &[(&str, u64)] = &[
    (
        "slices/arenas/events/sandworm/base_-_angry_earth_-_desert_scorpion_impaler_small_sandworm.dynamicslice",
        5626,
    ),
    (
        "slices/arenas/events/sandworm/base_-_angry_earth_-_desert_scorpion_slinger_sandworm.dynamicslice",
        5628,
    ),
    (
        "slices/arenas/events/sandworm/base_-_angry_earth_-_sand_elemental_shaman_sandworm.dynamicslice",
        11034,
    ),
    (
        "slices/arenas/events/sandworm/base_-_angryearth_-_sandelemental_heavy_sandworm.dynamicslice",
        2616,
    ),
    (
        "slices/catacombs/gameplay/spawners/pointspawners/enemyvariants/dg_catacombs_extraction_cryptkeeper.dynamicslice",
        6050,
    ),
    (
        "slices/catacombs/gameplay/spawners/pointspawners/enemyvariants/dg_catacombs_extraction_meatbag_green.dynamicslice",
        8050,
    ),
    (
        "slices/catacombs/gameplay/spawners/pointspawners/enemyvariants/dg_catacombs_extraction_mutated_nurse.dynamicslice",
        6067,
    ),
    (
        "slices/catacombs/gameplay/spawners/pointspawners/enemyvariants/dg_catacombs_extraction_soul_hunter.dynamicslice",
        6000,
    ),
    (
        "slices/characters/ancient_gargoyle_fodder.dynamicslice",
        1305,
    ),
    (
        "slices/characters/ancientguardian_javelineer.dynamicslice",
        7033,
    ),
    ("slices/characters/ancientguardian_mage.dynamicslice", 10020),
    (
        "slices/characters/ancientguardian_reaver.dynamicslice",
        7051,
    ),
    (
        "slices/characters/ancientguardian_spearman.dynamicslice",
        7063,
    ),
    (
        "slices/characters/anubianguardian_reaver.dynamicslice",
        7030,
    ),
    (
        "slices/characters/base_-_corrupted_-_corrupted_dynasty_warrior_empress_solo.dynamicslice",
        6017,
    ),
    ("slices/characters/blight_fiend.dynamicslice", 10548),
    (
        "slices/characters/catacombsboss/ancientguardianheavy/catacombs_ancientguardianheavy_boss.dynamicslice",
        19983,
    ),
    (
        "slices/characters/catacombsboss/ancientguardianheavy/catacombs_skeleton_1h_sword_minion_agheavy.dynamicslice",
        6050,
    ),
    (
        "slices/characters/catacombsboss/ancientguardianheavy/catacombs_skeleton_archer_minion_agheavy.dynamicslice",
        6034,
    ),
    (
        "slices/characters/catacombsboss/fulgoris/catacombs_fulgoris_boss.dynamicslice",
        14529,
    ),
    (
        "slices/characters/catacombsboss/nagacorrupted/catacombs_naga_corrupted_boss.dynamicslice",
        14480,
    ),
    (
        "slices/characters/catacombsnolos/catacombs_lost_feral_ghoul_nolos.dynamicslice",
        7068,
    ),
    (
        "slices/characters/corrupted_dynasty_musketeer.dynamicslice",
        6058,
    ),
    (
        "slices/characters/corrupted_dynasty_musketeer_empress.dynamicslice",
        6048,
    ),
    (
        "slices/characters/corrupted_dynasty_spearman.dynamicslice",
        6083,
    ),
    (
        "slices/characters/corrupted_dynasty_spearman_empress.dynamicslice",
        5984,
    ),
    (
        "slices/characters/corrupted_dynasty_summoner.dynamicslice",
        10003,
    ),
    (
        "slices/characters/corrupted_dynasty_summoner_empress.dynamicslice",
        9949,
    ),
    (
        "slices/characters/corrupted_dynasty_trainee.dynamicslice",
        6019,
    ),
    (
        "slices/characters/corrupted_dynasty_warrior.dynamicslice",
        6050,
    ),
    (
        "slices/characters/corrupted_dynasty_warrior_empress.dynamicslice",
        6052,
    ),
    (
        "slices/characters/corrupted_legion_sagittarii_female.dynamicslice",
        6047,
    ),
    (
        "slices/characters/corrupted_legion_signifer_male.dynamicslice",
        10051,
    ),
    ("slices/characters/corrupted_ogre.dynamicslice", 10017),
    (
        "slices/characters/corrupted_sailor_1h_club.dynamicslice",
        7053,
    ),
    (
        "slices/characters/corrupted_sailor_1h_sword.dynamicslice",
        7082,
    ),
    (
        "slices/characters/corrupted_sailor_1h_sword_shield.dynamicslice",
        6048,
    ),
    (
        "slices/characters/corrupted_sailor_2h_axe.dynamicslice",
        6049,
    ),
    (
        "slices/characters/corrupted_sailor_miner.dynamicslice",
        7018,
    ),
    (
        "slices/characters/corruptedlegion_cyclops.dynamicslice",
        14468,
    ),
    ("slices/characters/corruption_swarmer.dynamicslice", 7050),
    ("slices/characters/corruptionheavy_dt5.dynamicslice", 6035),
    ("slices/characters/damned_2h_spearman_a.dynamicslice", 6050),
    (
        "slices/characters/damned_2h_spearman_female.dynamicslice",
        6035,
    ),
    ("slices/characters/damned_acolyte.dynamicslice", 10053),
    ("slices/characters/damned_axeman.dynamicslice", 6034),
    ("slices/characters/damned_axeman_female.dynamicslice", 6050),
    ("slices/characters/damned_bowman.dynamicslice", 6036),
    ("slices/characters/damned_bowman_female.dynamicslice", 6051),
    (
        "slices/characters/damned_commander_thorpe.dynamicslice",
        4082,
    ),
    ("slices/characters/damned_cultist.dynamicslice", 6047),
    (
        "slices/characters/damned_cultist_isabella_msq2.dynamicslice",
        1035,
    ),
    (
        "slices/characters/damned_cultist_priest_minion.dynamicslice",
        6048,
    ),
    ("slices/characters/damned_great_axeman.dynamicslice", 6065),
    (
        "slices/characters/damned_great_axeman_female.dynamicslice",
        6067,
    ),
    ("slices/characters/damned_grunt.dynamicslice", 6066),
    (
        "slices/characters/damned_pistoleer_female.dynamicslice",
        6031,
    ),
    ("slices/characters/damned_priest.dynamicslice", 10051),
    (
        "slices/characters/damned_rifleman_female.dynamicslice",
        6031,
    ),
    (
        "slices/characters/damned_rifleman_medium.dynamicslice",
        6083,
    ),
    ("slices/characters/damned_swordsman.dynamicslice", 6078),
    (
        "slices/characters/damned_swordsman_female.dynamicslice",
        6047,
    ),
    (
        "slices/characters/damned_villager_axethrower.dynamicslice",
        6032,
    ),
    (
        "slices/characters/damned_villager_axethrower_female.dynamicslice",
        6015,
    ),
    (
        "slices/characters/damned_villager_cleaver.dynamicslice",
        6052,
    ),
    (
        "slices/characters/damned_villager_hammer.dynamicslice",
        6015,
    ),
    ("slices/characters/damned_villager_knife.dynamicslice", 6034),
    (
        "slices/characters/damned_villager_pickaxe.dynamicslice",
        6052,
    ),
    (
        "slices/characters/disabledgatherablevariants/wildlife_rat_plaguerat_nogatherable.dynamicslice",
        8689,
    ),
    ("slices/characters/dryad_archer.dynamicslice", 6030),
    ("slices/characters/dryad_fulgoris.dynamicslice", 12997),
    ("slices/characters/dryad_prowler.dynamicslice", 7037),
    ("slices/characters/dryad_shaman.dynamicslice", 9965),
    ("slices/characters/dryad_soldier.dynamicslice", 10831),
    (
        "slices/characters/dynasty_empress_pearl_pedestal.dynamicslice",
        1101,
    ),
    (
        "slices/characters/dynasty_empress_pearl_pedestal_caster.dynamicslice",
        0,
    ),
    (
        "slices/characters/dynasty_empress_solo_pearl_pedestal.dynamicslice",
        1052,
    ),
    (
        "slices/characters/dynasty_empress_solo_pearl_pedestal_phase2.dynamicslice",
        1033,
    ),
    ("slices/characters/dynasty_maiden.dynamicslice", 10000),
    ("slices/characters/dynastyheavy.dynamicslice", 6013),
    ("slices/characters/elephant_h.dynamicslice", 8034),
    ("slices/characters/evil_knight_bow.dynamicslice", 6014),
    ("slices/characters/evil_knight_caster.dynamicslice", 6050),
    ("slices/characters/evil_knight_commander.dynamicslice", 6036),
    ("slices/characters/evil_knight_great_axe.dynamicslice", 6037),
    ("slices/characters/evil_knight_heavy.dynamicslice", 6066),
    ("slices/characters/evil_knight_lifestaff.dynamicslice", 6036),
    (
        "slices/characters/evil_knight_longsword_fodder.dynamicslice",
        6086,
    ),
    ("slices/characters/evil_knight_mace_tank.dynamicslice", 6064),
    ("slices/characters/evil_knight_spear.dynamicslice", 6032),
    (
        "slices/characters/evil_knight_voidgauntlet.dynamicslice",
        6066,
    ),
    ("slices/characters/evil_knight_warhammer.dynamicslice", 6031),
    ("slices/characters/evil_knight_wolf.dynamicslice", 7162),
    (
        "slices/characters/expeditions/isleofnight/ion_lost_feral_ghoul_puzzle.dynamicslice",
        7035,
    ),
    (
        "slices/characters/expeditions/shattermtn/isabella_basilica_phase1/damned_cultist_isabella_lair_basilica_01.dynamicslice",
        6051,
    ),
    (
        "slices/characters/ftue_corrupted/corrupted_sailor_zone3_club.dynamicslice",
        5067,
    ),
    ("slices/characters/ghost.dynamicslice", 6016),
    ("slices/characters/ghost_plagued.dynamicslice", 5999),
    ("slices/characters/ghost_shackled_boss.dynamicslice", 6031),
    ("slices/characters/gorilla_fire_dungeon.dynamicslice", 15682),
    ("slices/characters/gorilla_h.dynamicslice", 9683),
    ("slices/characters/gorilla_ice.dynamicslice", 15667),
    (
        "slices/characters/gorilla_lightning_dungeon.dynamicslice",
        15684,
    ),
    ("slices/characters/hercyne_boar.dynamicslice", 8046),
    ("slices/characters/hercyne_boar_alpha.dynamicslice", 8066),
    ("slices/characters/hercyne_corvid.dynamicslice", 9998),
    (
        "slices/characters/hercyne_corvid_pounce.dynamicslice",
        10018,
    ),
    ("slices/characters/hercyne_reindeer.dynamicslice", 6082),
    (
        "slices/characters/hercyne_reindeer_alpha.dynamicslice",
        5982,
    ),
    ("slices/characters/hercyne_spider.dynamicslice", 10664),
    (
        "slices/characters/hercyne_spider_typhonminion.dynamicslice",
        10669,
    ),
    (
        "slices/characters/hercyne_wolf_typhonminion.dynamicslice",
        5070,
    ),
    (
        "slices/characters/icedryad_fiend_shivers.dynamicslice",
        4032,
    ),
    ("slices/characters/inquisitor_cleric.dynamicslice", 9998),
    (
        "slices/characters/inquisitor_flamekeeper.dynamicslice",
        8994,
    ),
    ("slices/characters/inquisitor_markswoman.dynamicslice", 5999),
    ("slices/characters/inquisitor_warden.dynamicslice", 7000),
    (
        "slices/characters/invasion/invasion_bomber.dynamicslice",
        6984,
    ),
    (
        "slices/characters/invasion/invasion_bomber_cowlauncher.dynamicslice",
        199,
    ),
    (
        "slices/characters/invasion/invasion_brute.dynamicslice",
        14498,
    ),
    (
        "slices/characters/invasion/invasion_grunt.dynamicslice",
        8014,
    ),
    (
        "slices/characters/invasion/invasion_grunt_cowlauncher.dynamicslice",
        7933,
    ),
    (
        "slices/characters/invasion/invasion_miniboss_commander.dynamicslice",
        21982,
    ),
    (
        "slices/characters/invasion/invasion_miniboss_priest.dynamicslice",
        10098,
    ),
    (
        "slices/characters/invasion/invasion_raider_spear.dynamicslice",
        6047,
    ),
    (
        "slices/characters/invasion/invasion_raider_sword.dynamicslice",
        6031,
    ),
    (
        "slices/characters/invasion/invasion_sniper.dynamicslice",
        6018,
    ),
    ("slices/characters/invasion_spriggan.dynamicslice", 14271),
    (
        "slices/characters/isabella_lair_basilica_phase1.dynamicslice",
        69,
    ),
    ("slices/characters/isabella_msq2.dynamicslice", 2033),
    ("slices/characters/lost_knight_tank.dynamicslice", 6016),
    ("slices/characters/lost_mutated_nurse.dynamicslice", 6032),
    ("slices/characters/lost_pikeman.dynamicslice", 6034),
    (
        "slices/characters/lost_pirate_spectral_1hsword.dynamicslice",
        6066,
    ),
    (
        "slices/characters/lost_pirate_spectral_2hspear.dynamicslice",
        6064,
    ),
    ("slices/characters/lost_undead_naga.dynamicslice", 6031),
    (
        "slices/characters/outpostrush/or_dryad_archer.dynamicslice",
        6001,
    ),
    (
        "slices/characters/priest_lesser_damned_hound.dynamicslice",
        8667,
    ),
    ("slices/characters/risen.dynamicslice", 8032),
    ("slices/characters/risen_brute_minion.dynamicslice", 8020),
    (
        "slices/characters/risen_corruption_entity.dynamicslice",
        7049,
    ),
    (
        "slices/characters/risen_corruption_impaler.dynamicslice",
        7047,
    ),
    ("slices/characters/risen_left_arm.dynamicslice", 8036),
    ("slices/characters/risen_regurgitator.dynamicslice", 8017),
    ("slices/characters/risen_right_arm.dynamicslice", 8032),
    (
        "slices/characters/seasons/season_04/ancient_ice_guardian_trial_boss_solo_daily.dynamicslice",
        23499,
    ),
    ("slices/characters/skeleton_1h_sword.dynamicslice", 6083),
    (
        "slices/characters/skeleton_1h_sword_minion_agheavy.dynamicslice",
        3502,
    ),
    ("slices/characters/skeleton_2h_sword.dynamicslice", 6071),
    (
        "slices/characters/skeleton_ancient_mage_miniboss_minion.dynamicslice",
        5880,
    ),
    (
        "slices/characters/skeleton_ancient_mage_minion.dynamicslice",
        6084,
    ),
    ("slices/characters/skeleton_archer.dynamicslice", 6050),
    ("slices/characters/skeleton_club.dynamicslice", 6062),
    ("slices/characters/skeleton_crawler.dynamicslice", 6065),
    ("slices/characters/skeleton_mage.dynamicslice", 8498),
    ("slices/characters/skeleton_spear.dynamicslice", 6066),
    (
        "slices/characters/spellslices/hercyne_reindeer_briarturret_2.dynamicslice",
        1054,
    ),
    (
        "slices/characters/spellslices/hercyne_reindeer_briarturret_alpha.dynamicslice",
        1068,
    ),
    (
        "slices/characters/spellslices/naga_corrupted/naga_corrupted_corruption_entity_minion.dynamicslice",
        7035,
    ),
    (
        "slices/characters/spellslices/seasons/season_04/ancient_ice_guardian_trial_solo_cturret.dynamicslice",
        5104,
    ),
    (
        "slices/characters/spellslices/seasons/season_04/ancient_ice_guardian_trial_solo_rturret.dynamicslice",
        4999,
    ),
    ("slices/characters/spider.dynamicslice", 10662),
    ("slices/characters/spider_melee.dynamicslice", 10647),
    ("slices/characters/spider_ranged.dynamicslice", 10681),
    ("slices/characters/swamp_dryad_archer.dynamicslice", 6001),
    ("slices/characters/swamp_dryad_beast.dynamicslice", 14517),
    ("slices/characters/swamp_dryad_prowler.dynamicslice", 1784),
    ("slices/characters/swamp_dryad_soldier.dynamicslice", 6037),
    ("slices/characters/swamp_fiend.dynamicslice", 10534),
    ("slices/characters/undead_berserker.dynamicslice", 6020),
    ("slices/characters/undead_bloated_corpse.dynamicslice", 7033),
    ("slices/characters/undead_brute.dynamicslice", 11483),
    ("slices/characters/undead_gravedigger.dynamicslice", 6065),
    ("slices/characters/undead_grenadier.dynamicslice", 15279),
    ("slices/characters/undead_hunter.dynamicslice", 6050),
    ("slices/characters/undead_javelineer.dynamicslice", 5118),
    ("slices/characters/undead_navigator.dynamicslice", 6052),
    ("slices/characters/undead_officer.dynamicslice", 6013),
    ("slices/characters/undead_pirate_brute.dynamicslice", 11195),
    ("slices/characters/undead_pistoleer.dynamicslice", 6047),
    ("slices/characters/undead_sailor_1h_club.dynamicslice", 7034),
    (
        "slices/characters/undead_sailor_1h_sword.dynamicslice",
        7065,
    ),
    (
        "slices/characters/undead_sailor_1h_sword_shield.dynamicslice",
        6075,
    ),
    ("slices/characters/undead_sailor_2h_axe.dynamicslice", 6048),
    ("slices/characters/undead_shaman.dynamicslice", 10049),
    (
        "slices/characters/unstable_swamp_fiend_minion.dynamicslice",
        3265,
    ),
    ("slices/characters/upyr_baroness.dynamicslice", 8000),
    ("slices/characters/upyr_consort.dynamicslice", 8031),
    ("slices/characters/upyr_mindthief.dynamicslice", 8064),
    ("slices/characters/upyr_thrall_aggressor.dynamicslice", 6057),
    ("slices/characters/upyr_thrall_grunt.dynamicslice", 5699),
    ("slices/characters/upyr_thrall_meatbag.dynamicslice", 8034),
    (
        "slices/characters/upyr_thrall_meatbag_green.dynamicslice",
        8033,
    ),
    (
        "slices/characters/upyr_thrall_meatbag_purple.dynamicslice",
        8034,
    ),
    (
        "slices/characters/upyr_thrall_soul_hunter.dynamicslice",
        6084,
    ),
    ("slices/characters/werebear.dynamicslice", 8068),
    ("slices/characters/werebear_crusher.dynamicslice", 8030),
    ("slices/characters/werebear_rabid.dynamicslice", 8053),
    ("slices/characters/wereraven.dynamicslice", 9977),
    ("slices/characters/wereraven_caster.dynamicslice", 10026),
    ("slices/characters/werewolf.dynamicslice", 6034),
    ("slices/characters/werewolf_alpha.dynamicslice", 6016),
    (
        "slices/characters/wex_dungeon_affiliation/ghost_plagued_dg_aff.dynamicslice",
        967,
    ),
    (
        "slices/characters/wex_dungeon_affiliation/risen_left_arm_dg_aff.dynamicslice",
        8048,
    ),
    (
        "slices/characters/wex_levelvariants/variants_17dw/pounce_lion_anubianpanther_memora.dynamicslice",
        330,
    ),
    (
        "slices/characters/wex_levelvariants/variants_17dw/upyr_meatbag_green_aggro.dynamicslice",
        8048,
    ),
    (
        "slices/characters/wex_levelvariants/variants_17dw/upyr_meatbag_red_aggro.dynamicslice",
        8049,
    ),
    ("slices/characters/withered_feculent.dynamicslice", 8032),
    ("slices/characters/withered_grunt.dynamicslice", 6048),
    (
        "slices/dungeon/raid_monarchbluff/encounter1_jester/characters/lost_twinjesters_stdclone_right.dynamicslice",
        2066,
    ),
    (
        "slices/dungeon/raid_monarchbluff/encounter2_heiressandpg/lost_palatinearcher.dynamicslice",
        47452,
    ),
    (
        "slices/dungeon/raid_monarchbluff/encounter2_heiressandpg/lost_palatineswordsman.dynamicslice",
        14516,
    ),
    (
        "slices/dungeon/raid_monarchbluff/ld_characters/ion_lost_bloodbeast.dynamicslice",
        6032,
    ),
    (
        "slices/dungeon/raid_monarchbluff/ld_characters/ion_lost_feral_ghoul.dynamicslice",
        7049,
    ),
    (
        "slices/dungeon/raid_monarchbluff/ld_characters/ion_lost_ghastlyhatchet.dynamicslice",
        6064,
    ),
    (
        "slices/dungeon/raid_monarchbluff/ld_characters/ion_lost_ghastlysniper_platforming.dynamicslice",
        6065,
    ),
    (
        "slices/dungeon/raid_monarchbluff/ld_characters/ion_lost_ghastlyvillager_shovel.dynamicslice",
        6051,
    ),
    (
        "slices/dungeon/raid_monarchbluff/ld_characters/ion_lost_knight_bow.dynamicslice",
        6034,
    ),
    (
        "slices/dungeon/raid_monarchbluff/ld_characters/ion_lost_monarch_elite_chapter2.dynamicslice",
        6066,
    ),
    (
        "slices/dungeon/raid_monarchbluff/ld_characters/ion_lost_soulharvester.dynamicslice",
        5315,
    ),
    (
        "slices/dungeon/raid_monarchbluff/ld_characters/ion_lost_undead_cryptkeeper.dynamicslice",
        6084,
    ),
    (
        "slices/dungeon/shattermountain/corruptedvillagers/dungeon_shattermtn00_damnedvillageraxethrowerf_passive.dynamicslice",
        6067,
    ),
    (
        "slices/dungeon/shattermountain/corruptedvillagers/dungeon_shattermtn00_damnedvillageraxethrowerm_passive.dynamicslice",
        6103,
    ),
    (
        "slices/dungeon/shattermountain/corruptedvillagers/dungeon_shattermtn00_damnedvillagercleaver_agressive.dynamicslice",
        5983,
    ),
    (
        "slices/dungeon/shattermountain/corruptedvillagers/dungeon_shattermtn00_damnedvillagercleaver_passive.dynamicslice",
        6048,
    ),
    (
        "slices/dungeon/shattermountain/corruptedvillagers/dungeon_shattermtn00_damnedvillagerhammer_agressive.dynamicslice",
        6052,
    ),
    (
        "slices/dungeon/shattermountain/corruptedvillagers/dungeon_shattermtn00_damnedvillagerhammer_passive.dynamicslice",
        6017,
    ),
    (
        "slices/dungeon/shattermountain/corruptedvillagers/dungeon_shattermtn00_damnedvillagerknife_aggressive.dynamicslice",
        6081,
    ),
    (
        "slices/dungeon/shattermountain/corruptedvillagers/dungeon_shattermtn00_damnedvillagerknife_passive.dynamicslice",
        6063,
    ),
    (
        "slices/dungeon/shattermountain/corruptedvillagers/dungeon_shattermtn00_damnedvillagerpickaxe_aggressive.dynamicslice",
        2896,
    ),
    (
        "slices/dungeon/shattermountain/corruptedvillagers/dungeon_shattermtn00_damnedvillagerpickaxe_passive.dynamicslice",
        6033,
    ),
    (
        "slices/dungeon/shattermountain/dungeon_shatteredmtn00_corruptedpriest_mine.dynamicslice",
        10003,
    ),
    (
        "slices/dungeon/shattermountain/dungeon_shattermtn_00_rifleman_largeaggro.dynamicslice",
        6062,
    ),
    (
        "slices/dunwood/ai/withered_grunt_inquisitor.dynamicslice",
        6082,
    ),
    (
        "slices/encounters/msq/ai_fl_adiana_shadow_phase2.dynamicslice",
        120,
    ),
];

/// How long the body of the creature of `slice` stays after its death, ms.
pub fn corpse_ms(slice: &str) -> u64 {
    CORPSE_MS
        .binary_search_by(|(known, _)| known.cmp(&slice))
        .map_or(DEFAULT_CORPSE_MS, |found| CORPSE_MS[found].1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_creature_s_body_stays_its_measured_time_else_the_default() {
        assert!(
            CORPSE_MS.windows(2).all(|pair| pair[0].0 < pair[1].0),
            "sorted"
        );
        assert_eq!(
            corpse_ms("slices/characters/corrupted_sailor_1h_club.dynamicslice"),
            7053
        );
        assert_eq!(
            corpse_ms("slices/characters/corrupted_sailor_2h_axe.dynamicslice"),
            6049
        );
        assert_eq!(
            corpse_ms("slices/characters/unknown.dynamicslice"),
            DEFAULT_CORPSE_MS
        );
    }
}
