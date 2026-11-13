// SPDX-License-Identifier: AGPL-3.0-or-later
//! What the game mode script of each expedition holds that the sim needs for its game mode
//! entity ([`super::expedition_mode`]): the objective's tasks, the run's time to beat, the
//! objective's id and instance, where the run's end leaves the script state, the delay of the
//! state after it and what a mutated run's normal kill scores (findings
//! `2026-11-13-catacomb-erosion.md`, sections C15 and C16).
//!
//! **Observed** (client): each expedition's script class (`DungeonShatteredObeliskData` and
//! eleven more, the constructors `FUN_140fafde0`, `FUN_1461e1920`, ...) pushes its objective's
//! tasks in this order, each an index byte and a localization key, and calls the objective
//! maker (vtable `+0x8e0`, `FUN_143248d10`) with the root index (Starstone 27). A task's id on
//! the wire is the CRC-32 of its key, the root's `ConsecutiveTask` (`91d8155f`). Every task id
//! the entity sent in the retail expedition ledgers is one of these keys' CRC (811 ids counted
//! once per ledger).
//!
//! **Observed** (ledgers, scratch `local-data/scratch-work/c15-expedition/runs/`): the
//! objective id and instance are the same in every run of one expedition (Starstone
//! `650374807`/`3812856275804359371` in runs 3, 30, 43 and 44); the run's end (the end time and
//! the final score) leaves the script state at [`ExpeditionScript::end_done`] (24 runs), and one
//! more state follows after [`ExpeditionScript::victory_delay_ms`] (the shortest of each
//! expedition's runs); a mutated run's normal kill points and kills for bonus are the score's
//! rise at each `f963dc0c` step and the count that set `6974d92c` (Empyrean run 16, Tempest
//! runs 20 and 34). The client's script data holds other values (`pointsPerEnemy` 50, Starstone
//! 20; `enemyMultBonus` 60, Starstone 100; the time to beat 2400 for all but four): the server's
//! own data differs, so the ledgers rule. **Inferred**: the ten expeditions with no mutated
//! capture score a normal kill as Tempest does ([`TEMPEST_KILL_POINTS`],
//! [`TEMPEST_KILLS_FOR_BONUS`]), the values the client's Starstone data holds too.
//!
//! **Observed** (scratch `local-data/scratch-work/c16-expedition/taskties.py`, the script state
//! steps of 29 ledgers beside the times a player's place entered each trigger area of the
//! level script): 34 steps of 7 expeditions meet a player's entry into one trigger area within
//! 0.6 s in every run that shows the step (2 to 5 runs each; Starstone 82.914 s state 1,
//! 83.030 s `AreaSealedChamber`): [`ExpeditionScript::area_tasks`]. Two of them meet two
//! overlapping areas at once (the Depths' `AreaR3TP2Exit` with `AreaGlyphEventFail`, and
//! `AreaThorpeFight` with `AreaThorpeFail`): the one that is no fail area is kept (Inferred).
//! The other steps meet no area in every run: encounter ends, interactions and events the
//! client does not get (not built; the run's end does them).
//! The objective id is the CRC of `SlayerScript` and a runtime entity id
//! (`FUN_143248d10`): no capture explains its number, so the observed one is kept.

/// A task that ends when a player stands in a trigger area of the level script
/// ([`nw_world::dungeon_scripts::LevelScript::trigger_area`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct AreaTask {
    /// The done task count while it waits (the script state before its end).
    pub(super) done: u8,
    /// The trigger area's entity name.
    pub(super) area: &'static str,
}

/// One expedition's script values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ExpeditionScript {
    /// The game mode id.
    pub(super) game_mode: &'static str,
    /// `timeToBeat`, seconds (Observed: the first state of every retail run).
    pub(super) time_to_beat_s: u64,
    /// The objective id of the game mode entity's objective (Observed).
    pub(super) objective: u32,
    /// The objective's instance (Observed).
    pub(super) instance: u64,
    /// The index of the objective's root task (Observed: the client's objective call).
    pub(super) root_index: u8,
    /// The tasks in the script's order: index and localization key (Observed: the client).
    pub(super) tasks: &'static [(u8, &'static str)],
    /// How many tasks are done when the run ends (Observed: the script state at the end time).
    pub(super) end_done: u8,
    /// The delay of the next task after the end, ms; `None`: no task follows (Observed: Amrine,
    /// whose four runs end at its last task but one).
    pub(super) victory_delay_ms: Option<u64>,
    /// The points of a mutated run's normal kill (module text).
    pub(super) kill_points: u64,
    /// The normal kills of a mutated run that score; the last sets the kill bonus (module text).
    pub(super) kills_for_bonus: u64,
    /// The tasks that end when a player stands in a trigger area (module text).
    pub(super) area_tasks: &'static [AreaTask],
    /// The script keeps the run's score and times (Observed: Amrine sent no value change in 4
    /// runs, every other expedition did).
    pub(super) scored: bool,
}

/// The points of a Tempest's Heart normal kill in a mutated run (Observed: +20 at each scoring
/// `f963dc0c` step of runs 20 and 34).
const TEMPEST_KILL_POINTS: u64 = 20;
/// Tempest's Heart's normal kills that score (Observed: the 100th set `6974d92c`, run 20
/// 8181.399 s; the 101st to 117th scored 0).
const TEMPEST_KILLS_FOR_BONUS: u64 = 100;

/// The script values of the twelve expeditions of build 25810808.
pub(super) const EXPEDITION_SCRIPTS: [ExpeditionScript; 12] = [
    // Amrine Excavation.
    ExpeditionScript {
        game_mode: "DungeonAmrine",
        time_to_beat_s: 1_200,
        objective: 3_960_058_765,
        instance: 483_597_892_780_232_859,
        root_index: 16,
        tasks: &[
            (1, "@dungeon_amrine_task_explore"),
            (2, "@dungeon_amrine_task_sealed_gate"),
            (3, "@dungeon_amrine_find_simons_investigation"),
            (5, "@dungeon_amrine_task_restore_bridge_scripts"),
            (6, "@dungeon_amrine_task_find_vestibule"),
            (7, "@dungeon_amrine_task_restore_bridge_glyphs"),
            (8, "@dungeon_amrine_task_defeat_vestibule_guardians"),
            (9, "@dungeon_amrine_task_activate_bridge"),
            (10, "@dungeon_amrine_task_defeat_grunt"),
            (11, "@dungeon_amrine_task_hallowed_candle"),
            (12, "@dungeon_amrine_task_defeat_foreman"),
            (13, "@dungeon_amrine_task_find_simongrey"),
            (14, "@dungeon_amrine_task_defeat_simongrey"),
            (15, "@dungeon_amrine_task_simons_discovery"),
        ],
        end_done: 13,
        victory_delay_ms: None,
        // Inferred (module text).
        kill_points: TEMPEST_KILL_POINTS,
        kills_for_bonus: TEMPEST_KILLS_FOR_BONUS,
        area_tasks: &[],
        scored: false,
    },
    // The Starstone Barrows.
    ExpeditionScript {
        game_mode: "DungeonShatteredObelisk",
        time_to_beat_s: 2_400,
        objective: 650_374_807,
        instance: 3_812_856_275_804_359_371,
        root_index: 27,
        tasks: &[
            (1, "@dungeon_everfall_task_explore"),
            (2, "@dungeon_everfall_task_sealedchamber"),
            (3, "@dungeon_everfall_task_sealedchamberexit"),
            (4, "@dungeon_everfall_task_traversal"),
            (5, "@dungeon_everfall_task_traversalexit"),
            (6, "@dungeon_everfall_task_ampitheater"),
            (7, "@dungeon_everfall_task_ampitheaterexit"),
            (8, "@dungeon_everfall_task_mut_approach_simon"),
            (9, "@dungeon_everfall_task_mut_defeat_simon"),
            (10, "@dungeon_everfall_task_mut_approach_foreman"),
            (11, "@dungeon_everfall_task_mut_defeat_foreman"),
            (12, "@dungeon_everfall_task_mut_exit"),
            (13, "@dungeon_everfall_task_obeliskbarrier"),
            (14, "@dungeon_everfall_task_obeliskbarrier"),
            (15, "@dungeon_everfall_task_obeliskbarrier"),
            (16, "@dungeon_everfall_task_obeliskstatuseffect"),
            (17, "@dungeon_everfall_task_obelisksearchkey"),
            (18, "@dungeon_everfall_task_obelisksearchkeyget"),
            (19, "@dungeon_everfall_task_obeliskfindbless"),
            (20, "@dungeon_everfall_task_obeliskfindblessactivated"),
            (21, "@dungeon_everfall_task_obeliskusekey"),
            (22, "@dungeon_everfall_task_bossdoorenter"),
            (23, "@dungeon_everfall_task_bossdoorapproach"),
            (24, "@dungeon_everfall_task_boss"),
            (25, "@dungeon_everfall_task_complete"),
            (26, "@dungeon_everfall_task_complete"),
        ],
        end_done: 24,
        victory_delay_ms: Some(1_020),
        // Inferred (module text).
        kill_points: TEMPEST_KILL_POINTS,
        kills_for_bonus: TEMPEST_KILLS_FOR_BONUS,
        area_tasks: &[
            AreaTask {
                done: 0,
                area: "AreaSealedChamber",
            },
            AreaTask {
                done: 2,
                area: "AreaForbiddenPassage",
            },
            AreaTask {
                done: 3,
                area: "AreaForbiddenPassageExit",
            },
            AreaTask {
                done: 6,
                area: "AreaObelisk",
            },
            AreaTask {
                done: 14,
                area: "AreaStatusEffect",
            },
            AreaTask {
                done: 21,
                area: "AreaBossApproach",
            },
            AreaTask {
                done: 22,
                area: "AreaBoss",
            },
        ],
        scored: true,
    },
    // The Depths.
    ExpeditionScript {
        game_mode: "DungeonRestlessShores01",
        time_to_beat_s: 2_400,
        objective: 3_105_997_767,
        instance: 9_788_660_262_788_316_467,
        root_index: 27,
        tasks: &[
            (2, "@dungeon_restlessshores01_task_break_in"),
            (3, "@dungeon_restlessshores01_task_find_azoth_seal"),
            (4, "@dungeon_restlessshores01_task_unlock_azoth_seal"),
            (5, "@dungeon_restlessshores01_task_activate_mechana"),
            (6, "@dungeon_restlessshores01_task_fight_defenses"),
            (7, "@dungeon_restlessshores01_task_unlock_sealed_door"),
            (8, "@dungeon_restlessshores01_task_search_thorpe"),
            (9, "@dungeon_restlessshores01_task_restore_bridge"),
            (10, "@dungeon_restlessshores01_task_explore_portal"),
            (11, "@dungeon_restlessshores01_task_survive_ambush"),
            (12, "@dungeon_restlessshores01_task_exit_ambush_area"),
            (13, "@dungeon_restlessshores01_task_unlock_mechana"),
            (14, "@dungeon_restlessshores01_task_unseal_gate"),
            (15, "@dungeon_restlessshores01_task_seal_restore_bridge"),
            (16, "@dungeon_restlessshores01_task_find_key_chamber"),
            (17, "@dungeon_restlessshores01_task_defeat_priest"),
            (18, "@dungeon_restlessshores01_task_return_chamber"),
            (19, "@dungeon_restlessshores01_task_break_in_chamber"),
            (20, "@dungeon_restlessshores01_task_use_portal"),
            (21, "@dungeon_restlessshores01_task_defeat_defenders"),
            (22, "@dungeon_restlessshores01_task_get_thru_door"),
            (23, "@dungeon_restlessshores01_task_enter_arena"),
            (24, "@dungeon_restlessshores01_task_defeat_thorpe"),
            (25, "@dungeon_restlessshores01_task_victory_celebration"),
            (26, "@dungeon_restlessshores01_task_return_shores"),
        ],
        end_done: 23,
        victory_delay_ms: Some(4_950),
        // Inferred (module text).
        kill_points: TEMPEST_KILL_POINTS,
        kills_for_bonus: TEMPEST_KILLS_FOR_BONUS,
        area_tasks: &[
            AreaTask {
                done: 0,
                area: "AreaObjFindAzothSeal",
            },
            AreaTask {
                done: 1,
                area: "AreaObjUnlockAzothSeal",
            },
            AreaTask {
                done: 4,
                area: "AreaObjUnlockSealDoor",
            },
            AreaTask {
                done: 7,
                area: "AreaObjExplorePortal",
            },
            AreaTask {
                done: 10,
                area: "AreaR3TP2Exit",
            },
            AreaTask {
                done: 14,
                area: "AreaObjDefeatPriest",
            },
            AreaTask {
                done: 16,
                area: "AreaCoreRoomTP01",
            },
            AreaTask {
                done: 18,
                area: "AreaR6TP00a",
            },
            AreaTask {
                done: 21,
                area: "AreaThorpeFight",
            },
        ],
        scored: true,
    },
    // Dynasty Shipyard.
    ExpeditionScript {
        game_mode: "DungeonEbonscale00",
        time_to_beat_s: 2_400,
        objective: 1_427_766_700,
        instance: 11_749_352_901_103_350_115,
        root_index: 18,
        tasks: &[
            (1, "@dungeon_ebonscale00_task_enter_cave"),
            (2, "@dungeon_ebonscale00_task_destroy_monolith"),
            (3, "@dungeon_ebonscale00_task_plant_kegs"),
            (4, "@dungeon_ebonscale00_task_place_rockets"),
            (5, "@dungeon_ebonscale00_task_to_courtyard"),
            (6, "@dungeon_ebonscale00_task_defeat_heavy"),
            (7, "@dungeon_ebonscale00_task_plant_keg1"),
            (8, "@dungeon_ebonscale00_task_defend_keg1"),
            (9, "@dungeon_ebonscale00_task_plant_keg2"),
            (10, "@dungeon_ebonscale00_task_defend_keg2"),
            (11, "@dungeon_ebonscale00_task_move_to_dock"),
            (12, "@dungeon_ebonscale00_task_defeat_boss"),
            (13, "@dungeon_ebonscale00_task_defeat_captain"),
            (14, "@dungeon_ebonscale00_task_find_empress"),
            (15, "@dungeon_ebonscale00_task_defeat_empress"),
            (16, "@dungeon_ebonscale00_task_leave_dungeon"),
            (17, "@dungeon_ebonscale00_task_leave_dungeon"),
        ],
        end_done: 15,
        victory_delay_ms: Some(7_020),
        // Inferred (module text).
        kill_points: TEMPEST_KILL_POINTS,
        kills_for_bonus: TEMPEST_KILLS_FOR_BONUS,
        area_tasks: &[
            AreaTask {
                done: 0,
                area: "AreaCaveEntrance",
            },
            AreaTask {
                done: 4,
                area: "AreaCourtyard",
            },
            AreaTask {
                done: 10,
                area: "AreaIsabella",
            },
            AreaTask {
                done: 13,
                area: "AreaEmpress",
            },
        ],
        scored: true,
    },
    // Garden of Genesis.
    ExpeditionScript {
        game_mode: "DungeonEdengrove00",
        time_to_beat_s: 1_800,
        objective: 405_724_038,
        instance: 7_198_467_369_654_310_299,
        root_index: 21,
        tasks: &[
            (1, "@dungeon_edengrove00_task_find_source"),
            (2, "@dungeon_edengrove00_task_explore_garden"),
            (3, "@dungeon_edengrove00_task_enter_nursery"),
            (4, "@dungeon_edengrove00_task_defeat_taxodious"),
            (5, "@dungeon_edengrove00_task_seek_shrine"),
            (6, "@dungeon_edengrove00_task_clear_hollow"),
            (7, "@dungeon_edengrove00_task_cleanse_seed"),
            (8, "@dungeon_edengrove00_task_defend_sapling"),
            (9, "@dungeon_edengrove00_task_gather_sapling"),
            (10, "@dungeon_edengrove00_task_find_the_purifying_flame"),
            (11, "@dungeon_edengrove00_task_defeat_caretaker"),
            (12, "@dungeon_edengrove00_task_start_the_purifying_flame"),
            (13, "@dungeon_edengrove00_task_recover_flame"),
            (14, "@dungeon_edengrove00_task_locate_heart"),
            (15, "@dungeon_edengrove00_task_find_source_2"),
            (16, "@dungeon_edengrove00_task_defeat_fulgda"),
            (17, "@dungeon_edengrove00_task_find_heart"),
            (18, "@dungeon_edengrove00_task_defeat_greenskeeper"),
            (19, "@dungeon_edengrove_task_victory_celebration"),
            (20, "@dungeon_edengrove00_task_use_portalstone"),
        ],
        end_done: 18,
        victory_delay_ms: Some(5_020),
        // Inferred (module text).
        kill_points: TEMPEST_KILL_POINTS,
        kills_for_bonus: TEMPEST_KILLS_FOR_BONUS,
        area_tasks: &[
            AreaTask {
                done: 0,
                area: "AreaArboretumEntry",
            },
            AreaTask {
                done: 9,
                area: "AreaBogmonster",
            },
        ],
        scored: true,
    },
    // Lazarus Instrumentality.
    ExpeditionScript {
        game_mode: "DungeonReekwater00",
        time_to_beat_s: 2_100,
        objective: 3_523_980_708,
        instance: 2_638_339_741_526_741_819,
        root_index: 25,
        tasks: &[
            (1, "@dungeon_reekwater_task_entry_01"),
            (2, "@dungeon_reekwater_task_entry_02"),
            (3, "@dungeon_reekwater_task_ostium"),
            (4, "@dungeon_reekwater_task_lazarus_01"),
            (5, "@dungeon_reekwater_task_lazarus_02"),
            (6, "@dungeon_reekwater_task_lazarus_03"),
            (7, "@dungeon_reekwater_task_tribute_01"),
            (8, "@dungeon_reekwater_task_tribute_01b"),
            (9, "@dungeon_reekwater_task_tribute_02"),
            (10, "@dungeon_reekwater_task_tribute_03"),
            (11, "@dungeon_reekwater_task_boss_naga"),
            (12, "@dungeon_reekwater_task_walk_01"),
            (13, "@dungeon_reekwater_task_walk_02"),
            (14, "@dungeon_reekwater_task_control_01"),
            (15, "@dungeon_reekwater_task_control_02"),
            (16, "@dungeon_reekwater_task_control_03"),
            (17, "@dungeon_reekwater_task_control_04"),
            (18, "@dungeon_reekwater_task_control_05"),
            (19, "@dungeon_reekwater_task_control_06"),
            (20, "@dungeon_reekwater_task_control_07"),
            (21, "@dungeon_reekwater_task_boss_01"),
            (22, "@dungeon_reekwater_task_boss_02"),
            (23, "@dungeon_reekwater_task_victory_celebration"),
            (24, "@dungeon_reekwater_task_complete"),
        ],
        end_done: 22,
        victory_delay_ms: Some(9_000),
        // Inferred (module text).
        kill_points: TEMPEST_KILL_POINTS,
        kills_for_bonus: TEMPEST_KILLS_FOR_BONUS,
        area_tasks: &[
            AreaTask {
                done: 0,
                area: "AreaOstium",
            },
            AreaTask {
                done: 3,
                area: "AreaWell01",
            },
            AreaTask {
                done: 13,
                area: "AreaLastWalkExit",
            },
            AreaTask {
                done: 15,
                area: "AreaControlRoomUpper",
            },
        ],
        scored: true,
    },
    // Barnacles and Black Powder.
    ExpeditionScript {
        game_mode: "DungeonCutlassKeys00",
        time_to_beat_s: 2_700,
        objective: 65_364_582,
        instance: 1_875_509_471_452_988_795,
        root_index: 30,
        tasks: &[
            (1, "@dungeon_cutlasskeys00_task_explore"),
            (2, "@dungeon_cutlasskeys00_task_explore_talk"),
            (3, "@dungeon_cutlasskeys00_task_open_entry_gate"),
            (4, "@dungeon_cutlasskeys00_task_enter_hideout"),
            (5, "@dungeon_cutlasskeys00_task_destroy_barrier"),
            (6, "@dungeon_cutlasskeys00_task_dockpassage"),
            (7, "@dungeon_cutlasskeys00_task_find_lever_docks"),
            (8, "@dungeon_cutlasskeys00_task_enter_passage"),
            (9, "@dungeon_cutlasskeys00_task_open_blocked_passage"),
            (10, "@dungeon_cutlasskeys00_task_defeat_alligators"),
            (11, "@dungeon_cutlasskeys00_task_find_explosives"),
            (12, "@dungeon_cutlasskeys00_task_pickup_explosives"),
            (13, "@dungeon_cutlasskeys00_task_return_explosives"),
            (14, "@dungeon_cutlasskeys00_task_detonate_explosives"),
            (15, "@dungeon_cutlasskeys00_task_go_to_docks"),
            (16, "@dungeon_cutlasskeys00_task_defeat_pirates"),
            (17, "@dungeon_cutlasskeys00_task_enter_ship"),
            (18, "@dungeon_cutlasskeys00_task_defeat_pirates_ship"),
            (19, "@dungeon_cutlasskeys00_task_exit_ship"),
            (20, "@dungeon_cutlasskeys00_task_defeat_admiral"),
            (21, "@dungeon_cutlasskeys00_task_exit_pirate_cove"),
            (22, "@dungeon_cutlasskeys00_task_enter_dead_mans_retreat"),
            (23, "@dungeon_cutlasskeys00_task_defeat_berserker"),
            (24, "@dungeon_cutlasskeys00_task_talk_mariner"),
            (25, "@dungeon_cutlasskeys00_task_open_siren_gate"),
            (26, "@dungeon_cutlasskeys00_task_survive_depths"),
            (27, "@dungeon_cutlasskeys00_task_defeat_siren_boss"),
            (28, "@dungeon_cutlasskeys00_task_complete"),
            (29, "@dungeon_cutlasskeys00_task_complete"),
        ],
        end_done: 27,
        victory_delay_ms: Some(9_510),
        // Inferred (module text).
        kill_points: TEMPEST_KILL_POINTS,
        kills_for_bonus: TEMPEST_KILLS_FOR_BONUS,
        area_tasks: &[],
        scored: true,
    },
    // Empyrean Forge.
    ExpeditionScript {
        game_mode: "DungeonGreatCleave01",
        time_to_beat_s: 2_400,
        objective: 2_740_710_793,
        instance: 2_179_753_549_594_230_851,
        root_index: 22,
        tasks: &[
            (1, "@dungeon_greatcleave01_task_explore"),
            (2, "@dungeon_greatcleave01_task_solveforgeentry"),
            (3, "@dungeon_greatcleave01_task_gain_access_forge"),
            (4, "@dungeon_greatcleave01_task_unlock_dial"),
            (5, "@dungeon_greatcleave01_task_talk_to_nora"),
            (6, "@dungeon_greatcleave01_task_power_the_forge"),
            (7, "@dungeon_greatcleave01_task_solve_the_mechanism"),
            (8, "@dungeon_greatcleave01_task_primordial"),
            (9, "@dungeon_greatcleave01_task_pursue_corrupted"),
            (10, "@dungeon_greatcleave01_task_theheel"),
            (11, "@dungeon_greatcleave01_task_forge"),
            (12, "@dungeon_greatcleave01_task_ifrit"),
            (13, "@dungeon_greatcleave01_task_wayforward"),
            (14, "@dungeon_greatcleave01_task_investigate"),
            (16, "@dungeon_greatcleave01_task_defeat"),
            (17, "@dungeon_greatcleave01_task_varangians"),
            (18, "@dungeon_greatcleave01_task_commanders"),
            (19, "@dungeon_greatcleave01_task_defeattheknight"),
            (20, "@dungeon_greatcleave01_victory_cutscene"),
            (21, "@dungeon_greatcleave01_task_exit_dungeon"),
        ],
        end_done: 19,
        victory_delay_ms: Some(5_030),
        // Observed: +24 a kill, the 85th kill's bonus, the 86th to 97th 0 (run 16).
        kill_points: 24,
        kills_for_bonus: 85,
        area_tasks: &[],
        scored: true,
    },
    // Tempest's Heart.
    ExpeditionScript {
        game_mode: "DungeonShatterMtn00",
        time_to_beat_s: 3_600,
        objective: 3_709_721_095,
        instance: 629_729_803_651_660_827,
        root_index: 35,
        tasks: &[
            (1, "@dungeon_shattermtn00_task_explore_entry"),
            (2, "@dungeon_shattermtn00_task_open_gate_madness"),
            (3, "@dungeon_shattermtn00_task_continue_descent"),
            (4, "@dungeon_shattermtn00_task_hold_against_corrupted"),
            (5, "@dungeon_shattermtn00_task_defeat_spriggan"),
            (6, "@dungeon_shattermtn00_task_ascend_watchtower"),
            (7, "@dungeon_shattermtn00_task_enter_portal_watchtower"),
            (8, "@dungeon_shattermtn00_task_void_tunnel_01"),
            (9, "@dungeon_shattermtn00_task_get_cellar_key"),
            (10, "@dungeon_shattermtn00_task_exit_void_tunnel_01"),
            (11, "@dungeon_shattermtn00_task_use_cellar_key"),
            (12, "@dungeon_shattermtn00_task_navigate_cave"),
            (13, "@dungeon_shattermtn00_task_continue_graveyard"),
            (14, "@dungeon_shattermtn00_task_open_gate_basilica"),
            (15, "@dungeon_shattermtn00_task_defeat_isabella_basilica_entry"),
            (16, "@dungeon_shattermtn00_task_pursue_isabella_basilica"),
            (17, "@dungeon_shattermtn00_task_survive_basilica"),
            (18, "@dungeon_shattermtn00_task_pursue_isabella_chandeliers"),
            (19, "@dungeon_shattermtn00_task_defeat_isabella_chandeliers"),
            (20, "@dungeon_shattermtn00_task_exit_basilica"),
            (21, "@dungeon_shattermtn00_task_search_relic"),
            (22, "@dungeon_shattermtn00_task_use_staff_relic"),
            (23, "@dungeon_shattermtn00_task_cross_corrupted_sea"),
            (24, "@dungeon_shattermtn00_task_return_relic"),
            (25, "@dungeon_shattermtn00_task_explore_city"),
            (26, "@dungeon_shattermtn00_task_defeat_ogre"),
            (27, "@dungeon_shattermtn00_task_enter_portal_city"),
            (28, "@dungeon_shattermtn00_task_void_tunnel_02"),
            (29, "@dungeon_shattermtn00_task_defeat_naga"),
            (30, "@dungeon_shattermtn00_task_enter_portal_naga"),
            (31, "@dungeon_shattermtn00_task_void_tunnel_03"),
            (32, "@dungeon_shattermtn00_task_defeat_isabella_final"),
            (33, "@dungeon_shattermtn00_task_victory_celebration"),
            (34, "@dungeon_shattermtn00_task_dungeon_complete"),
        ],
        end_done: 32,
        victory_delay_ms: Some(1_030),
        kill_points: TEMPEST_KILL_POINTS,
        kills_for_bonus: TEMPEST_KILLS_FOR_BONUS,
        area_tasks: &[
            AreaTask {
                done: 11,
                area: "AreaCellarExit",
            },
            AreaTask {
                done: 12,
                area: "AreaBasilicaCourtyard",
            },
            AreaTask {
                done: 13,
                area: "AreaBasilicaGate",
            },
            AreaTask {
                done: 15,
                area: "AreaBasilicaAlter",
            },
            AreaTask {
                done: 24,
                area: "AreaCityStatue",
            },
        ],
        scored: true,
    },
    // The Ennead.
    ExpeditionScript {
        game_mode: "DungeonBrimstoneSands00",
        time_to_beat_s: 3_300,
        objective: 2_736_271_093,
        instance: 5_055_147_868_575_381_779,
        root_index: 11,
        tasks: &[
            (1, "@dungeon_brimstone_task_defeat_shai"),
            (2, "@dungeon_brimstone_task_reach_scarab"),
            (3, "@dungeon_brimstone_task_defeat_scarab_boss"),
            (4, "@dungeon_brimstone_task_open_corrupted_gate"),
            (5, "@dungeon_brimstone_task_battle_through_legion"),
            (6, "@dungeon_brimstone_task_defeat_crassus"),
            (7, "@dungeon_brimstone_task_reach_apex_of_ennead"),
            (8, "@dungeon_brimstone_task_defeat_goliaths"),
            (9, "@dungeon_brimstone_task_victory_celebration"),
            (10, "@dungeon_brimstone_task_exit_expedition"),
        ],
        end_done: 8,
        victory_delay_ms: Some(4_970),
        // Inferred (module text).
        kill_points: TEMPEST_KILL_POINTS,
        kills_for_bonus: TEMPEST_KILLS_FOR_BONUS,
        area_tasks: &[],
        scored: true,
    },
    // Savage Divide.
    ExpeditionScript {
        game_mode: "DungeonFirstLight01",
        time_to_beat_s: 2_700,
        objective: 1_404_333_755,
        instance: 6_374_506_798_861_968_947,
        root_index: 25,
        tasks: &[
            (1, "@dungeon_firstlight01_task_explore"),
            (2, "@dungeon_firstlight01_task_trainorb"),
            (3, "@dungeon_firstlight01_task_gotohub1"),
            (4, "@dungeon_firstlight01_task_2b_unlock"),
            (5, "@dungeon_firstlight01_task_2b_progress"),
            (6, "@dungeon_firstlight01_task_2b_gate"),
            (7, "@dungeon_firstlight01_task_2b_pressureplates"),
            (8, "@dungeon_firstlight01_task_2b_findorb"),
            (9, "@dungeon_firstlight01_task_hub1orb1"),
            (10, "@dungeon_firstlight01_task_2a_unlock"),
            (11, "@dungeon_firstlight01_task_2a_progress"),
            (12, "@dungeon_firstlight01_task_2a_gate"),
            (13, "@dungeon_firstlight01_task_2a_findorb"),
            (14, "@dungeon_firstlight01_task_hub1orb2"),
            (15, "@dungeon_firstlight01_task_explorefirstcaptain"),
            (16, "@dungeon_firstlight01_task_firstcaptain"),
            (17, "@dungeon_firstlight01_task_explorenextcaptain"),
            (18, "@dungeon_firstlight01_task_hub2orb1"),
            (19, "@dungeon_firstlight01_task_azothbridge"),
            (20, "@dungeon_firstlight01_task_hub2orb2"),
            (21, "@dungeon_firstlight01_task_uphill"),
            (22, "@dungeon_firstlight01_task_secondcaptain"),
            (23, "@dungeon_firstlight01_task_exit"),
            (24, "@dungeon_firstlight01_task_exit"),
        ],
        end_done: 22,
        victory_delay_ms: Some(1_540),
        // Inferred (module text).
        kill_points: TEMPEST_KILL_POINTS,
        kills_for_bonus: TEMPEST_KILLS_FOR_BONUS,
        area_tasks: &[
            AreaTask {
                done: 4,
                area: "Area_2b_Lotus",
            },
            AreaTask {
                done: 10,
                area: "Area_2a_Dragon",
            },
            AreaTask {
                done: 14,
                area: "Area_Approach_Boss_1",
            },
        ],
        scored: true,
    },
    // Glacial Tarn.
    ExpeditionScript {
        game_mode: "DungeonGreatCleave00",
        time_to_beat_s: 2_400,
        objective: 3_352_378_451,
        instance: 10_359_328_732_029_568_963,
        root_index: 19,
        tasks: &[
            (1, "@dungeon_greatcleave00_task_explore"),
            (2, "@dungeon_greatcleave00_task_flame"),
            (3, "@dungeon_greatcleave00_task_upper"),
            (4, "@dungeon_greatcleave00_task_ice"),
            (5, "@dungeon_greatcleave00_task_further"),
            (6, "@dungeon_greatcleave00_task_troll"),
            (7, "@dungeon_greatcleave00_task_cross"),
            (8, "@dungeon_greatcleave00_task_break"),
            (9, "@dungeon_greatcleave00_task_seal"),
            (10, "@dungeon_greatcleave00_task_bridge"),
            (11, "@dungeon_greatcleave00_task_together"),
            (12, "@dungeon_greatcleave00_task_forge"),
            (13, "@dungeon_greatcleave00_task_amalgam"),
            (14, "@dungeon_greatcleave00_task_magick"),
            (15, "@dungeon_greatcleave00_task_power"),
            (16, "@dungeon_greatcleave00_task_loth"),
            (17, "@dungeon_greatcleave00_task_cutscene"),
            (18, "@dungeon_greatcleave00_task_exit_dungeon"),
        ],
        end_done: 16,
        victory_delay_ms: Some(1_530),
        // Inferred (module text).
        kill_points: TEMPEST_KILL_POINTS,
        kills_for_bonus: TEMPEST_KILLS_FOR_BONUS,
        area_tasks: &[],
        scored: true,
    },
];

/// The script values of game mode `game_mode` (any case); `None` for a game mode that is no
/// expedition of the table.
pub(super) fn expedition_script(game_mode: &str) -> Option<&'static ExpeditionScript> {
    EXPEDITION_SCRIPTS
        .iter()
        .find(|script| script.game_mode.eq_ignore_ascii_case(game_mode))
}

#[cfg(test)]
mod tests {
    use nw_formats::objectstream::lowercase_crc32;

    use super::*;

    /// Retail Starstone (runs 3 and 30): the root task 27, task 1 `bc60636e` (explore), task 13
    /// to 15 `bd0ac036` (the obelisk barrier, three times) and task 25 `6327af6c` (complete).
    #[test]
    fn the_starstone_tasks_are_the_retail_task_ids() {
        let script = expedition_script("dungeonshatteredobelisk").expect("Starstone");
        assert_eq!(script.root_index, 27);
        let id = |index: u8| {
            script
                .tasks
                .iter()
                .find(|(task, _)| *task == index)
                .map(|(_, key)| lowercase_crc32(key))
        };
        assert_eq!(id(1), Some(0xbc60_636e));
        assert_eq!(id(13), Some(0xbd0a_c036));
        assert_eq!(id(15), Some(0xbd0a_c036));
        assert_eq!(id(25), Some(0x6327_af6c));
    }

    /// Every script's tasks rise in index and stay below the root; its end leaves at least one
    /// task undone when a task follows the end.
    #[test]
    fn every_script_is_ordered_below_its_root() {
        for script in &EXPEDITION_SCRIPTS {
            assert!(
                script.tasks.windows(2).all(|pair| pair[0].0 < pair[1].0),
                "{}",
                script.game_mode
            );
            assert!(script.tasks.iter().all(|(index, _)| *index < script.root_index));
            let after = usize::from(script.end_done) + usize::from(script.victory_delay_ms.is_some());
            assert!(after <= script.tasks.len(), "{}", script.game_mode);
        }
        assert!(expedition_script("DungeonDunwood02").is_none(), "no retail run");
    }

    /// Every area task waits before the run's end, in rising order; 34 retail ties of 7
    /// expeditions (Starstone's first: `AreaSealedChamber` ends task 1).
    #[test]
    fn the_area_tasks_end_before_the_run_does() {
        for script in &EXPEDITION_SCRIPTS {
            assert!(
                script.area_tasks.windows(2).all(|pair| pair[0].done < pair[1].done),
                "{}",
                script.game_mode
            );
            assert!(
                script.area_tasks.iter().all(|task| task.done < script.end_done),
                "{}",
                script.game_mode
            );
        }
        let ties: usize = EXPEDITION_SCRIPTS
            .iter()
            .map(|script| script.area_tasks.len())
            .sum();
        assert_eq!(ties, 34);
        let starstone = expedition_script("DungeonShatteredObelisk").expect("Starstone");
        assert_eq!(
            starstone.area_tasks[0],
            AreaTask {
                done: 0,
                area: "AreaSealedChamber"
            }
        );
    }

    /// Retail mutated runs: Empyrean (run 16) scores +24 a kill up to 85, Tempest (runs 20 and
    /// 34) +20 up to 100.
    #[test]
    fn the_mutated_kill_scores_are_the_retail_ones() {
        let score = |mode: &str| {
            let script = expedition_script(mode).expect("script");
            (script.kill_points, script.kills_for_bonus)
        };
        assert_eq!(score("DungeonGreatCleave01"), (24, 85));
        assert_eq!(score("DungeonShatterMtn00"), (20, 100));
    }
}
