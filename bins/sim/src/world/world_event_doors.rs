// SPDX-License-Identifier: AGPL-3.0-or-later
//! The doors of the open world's world events (door study 2026-11-12, [`nw_messages::door`]),
//! checked against the content files: each runs as an open world encounter
//! ([`super::dungeon_script`]), shows its root ([`super::encounter_roots`]) and spawns its door
//! objects as encounter objects. Nothing here runs in the sim: it is the tests of that chain.
//!
//! **The holdout** (Observed, retail `99a_msq_01b_learnsoulwardens`; the slice
//! `slices/pois/ancient/design/holdout/enc_worldevent_holdout_setup`): the spawn group
//! `WorldEvent_SpawnGroup_Single_Master_448598026` spawns it by a path that is out of date (the
//! GUID's path is in the `holdout` folder), so the content tool reads it by its GUID. Its
//! manager runs `Wave_00 - Setup` (3 s), `Wave_01 - Interact` (the glyph
//! `worldevent_holdout_interactobject_master`, gatherable `Holdout_Interact`, and the solid door
//! `worldevent_object_holdout_door_solid`), `Wave_02 - Holdout` (60 s; the door animation
//! `worldevent_object_holdout_door_anim` and the one-way door
//! `worldevent_object_holdout_oneway_setup`; its child `Subwave_AreaSpawner` spawns the guardians
//! for 60 s), `Wave_03 - Get Out` (60 s) and `Wave_04 - Cleanup` (3 s). Its six stage states are
//! the root's components 5 to 10 in the slice's stream order (Cleanup, Interact, Setup, Holdout,
//! Get Out, Subwave): at 180.442 s Setup is done and Interact runs; the glyph is gathered at
//! 242.566 s (3746); at 245.585 s Interact is done and Holdout and Subwave run; at 245.635 s the
//! solid door is gone and the one-way door spawns: variation `01 02 02 aa`, position, gatherable
//! controller 4 and door `Closed` on components 1, 2, 3 and 7, at (8550.913, 4278.253), turned
//! 90 degrees clockwise from the encounter; at 305.600 s Holdout and Subwave are done and Get
//! Out runs. The door's slot stopped at 378.650 s, 10 s after the end the stage times give (the
//! spawner's respawn, [`super::dungeon_script`]).
//!
//! **The stronghold siren** (Observed, retail `Playthrough_Part_18` 4558.190 s; the slice
//! `slices/worldevents/elite_stronghold_siege/siren/enc_worldevent_stronghold_siren`): no client
//! data places it, so the content tool places it at its retail root
//! (`RETAIL_PLACED_ENCOUNTERS` of the content tool). Its two doors are no entities of their own:
//! they are the slice's `Door_Teleport` entities, members 67 and 71 of the root group, `Closed`,
//! and a player passes them by the door interaction (kind 67, [`super::travel`]).
//!
//! Observed difference (not changed here): retail destroyed the gathered glyph 3 s after the
//! gather (gatherable 8 and a 3 s delayed event at 242.570 s, slot stop at 245.585 s), and the
//! Interact stage ended with it; this server's gathering removes a start object at the gather
//! ([`super::gathering`]), so the stage ends and the door spawns 3 s earlier.

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};

    use nw_content::town_props::{TownProps, encounter_objects_path, encounter_roots_path};
    use nw_messages::position::{PositionInTheWorldReplicatedState, RotationComponent};
    use nw_messages::{BundleGroup, MemberState};
    use nw_sim_core::Position;
    use nw_world::dungeon_scripts::{
        DungeonScripts, Encounter, LevelScript, OPEN_WORLD_LEVEL, TargetMob, dungeon_scripts_path,
    };
    use nw_world::records::PatrolPath;

    use crate::world::dungeon_script::{
        DungeonRun, OBJECT_HEALTH, OpenWorldRules, ScriptHost, ScriptView,
    };
    use crate::world::encounter_roots::{
        ENCOUNTER_OBJECT_ENTITY_BASE, ENCOUNTER_ROOT_ENTITY_BASE, EncounterObjects, EncounterRoots,
        STATUS_IN_PROGRESS, STATUS_SUCCEEDED,
    };
    use crate::world::slots::Slot;

    const BUILD: &str = "25810808";
    /// The holdout's encounter slice.
    const HOLDOUT_SLICE: &str =
        "slices/pois/ancient/design/holdout/enc_worldevent_holdout_setup.dynamicslice";
    /// The retail root's place and turn (Observed, 180.442 s: x, y and the rotation's z).
    const ROOT_X: f32 = 8536.872;
    const ROOT_Y: f32 = 4277.3486;
    const ROOT_ROTATION_Z: u16 = 34196;
    /// The root's position component (Observed).
    const ROOT_POSITION_COMPONENT: u64 = 3;
    /// The asset ids of the glyph, the solid door, the door animation and the one-way door.
    const GLYPH: &str = "7b34ec22115254cc843a326be210eac6";
    const SOLID_DOOR: &str = "8a2a7910e55f59829aa7ed0e20395355";
    const DOOR_ANIM: &str = "6ab34715226a58ceb7e3f58ceb6c4871";
    const ONE_WAY_DOOR: &str = "3c3da2f604a75c39a2a16e0fdd1add89";
    /// The retail one-way door (245.635 s): its variation, gatherable controller and door
    /// members, its place and the rotation's z.
    const RETAIL_VARIATION: &str = "010e010202aa";
    const RETAIL_GATHERABLE: &str = "030c010104";
    const RETAIL_DOOR: &str = "079a24010101";
    /// The one-way door's components (Observed): variation, position, gatherable, door.
    const DOOR_VARIATION_COMPONENT: u64 = 1;
    const DOOR_POSITION_COMPONENT: u64 = 2;
    const DOOR_GATHERABLE_COMPONENT: u64 = 3;
    const DOOR_COMPONENT: u64 = 7;
    /// The door animation's position component (Observed).
    const ANIM_POSITION_COMPONENT: u64 = 1;
    const DOOR_X: f32 = 8550.913;
    const DOOR_Y: f32 = 4278.253;
    const DOOR_Z_RAW: u16 = 8495;
    /// The door's height in metres (the raw height 8495), where the test player stands.
    const DOOR_Z_M: f32 = 172.22;
    const DOOR_ROTATION_Z: u16 = 10630;
    /// The retail door animation's place and the rotation's z (245.635 s).
    const ANIM_X: f32 = 8553.375;
    const ANIM_Y: f32 = 4278.6416;
    const ANIM_ROTATION_Z: u16 = 56926;
    /// The retail stage states of the root's components 5 to 10 (whole members).
    const STAGES_INTERACT: [&str; 6] = [
        "05952101010200000000000000000000000100000001",
        "069521010103000000000000000000000001000000010000000200000002",
        "0795210101040000000000000000000000010000000100000002000000020000000300000003",
        "08952101010200000000000000000000000100000001",
        "09952101010200000000000000000000000100000001",
        "0a952101010200000000000000000000000100000001",
    ];
    const STAGES_HOLDOUT: [&str; 6] = [
        "05952101010200000000000000000000000100000001",
        "0695210101040000000000000000000000010000000100000002000000020000000300000003",
        "0795210101040000000000000000000000010000000100000002000000020000000300000003",
        "089521010103000000000000000000000001000000010000000200000002",
        "09952101010200000000000000000000000100000001",
        "0a9521010103000000000000000000000001000000010000000200000002",
    ];
    const STAGES_GET_OUT: [&str; 6] = [
        "05952101010200000000000000000000000100000001",
        "0695210101040000000000000000000000010000000100000002000000020000000300000003",
        "0795210101040000000000000000000000010000000100000002000000020000000300000003",
        "0895210101040000000000000000000000010000000100000002000000020000000300000003",
        "099521010103000000000000000000000001000000010000000200000002",
        "0a95210101040000000000000000000000010000000100000002000000020000000300000003",
    ];
    /// The stage times (Observed: the slice's objectives; retail 245.585 to 305.600 s).
    const SETUP_MS: u64 = 3_000;
    const HOLDOUT_MS: u64 = 60_000;
    const GET_OUT_MS: u64 = 60_000;
    const CLEANUP_MS: u64 = 3_000;
    /// The spawner's respawn (the range 10 to 0 s).
    const RESPAWN_MS: u64 = 10_000;
    /// The stronghold siren's encounter slice, its retail root's place (x, y, raw height;
    /// `Playthrough_Part_18` 4558.190 s), its stage count and its two teleport doors (whole
    /// members on the root, components 67 and 71).
    const SIREN_SLICE: &str = "slices/worldevents/elite_stronghold_siege/siren/enc_worldevent_stronghold_siren.dynamicslice";
    const SIREN_X: f32 = 11_490.633;
    const SIREN_Y: f32 = 2_738.581_3;
    const SIREN_Z_RAW: u16 = 4606;
    const SIREN_STAGES: usize = 18;
    const SIREN_DOORS: [(u64, &str); 2] = [(67, "439a24010101"), (71, "479a24010101")];
    /// One sim tick.
    const TICK_MS: u64 = 100;
    const T0: u64 = 1_000;
    /// A view radius and the open world rules of the test.
    const RADIUS_M: f32 = 120.0;
    const RULES: OpenWorldRules = OpenWorldRules {
        near_m: RADIUS_M,
        respawn_ms: 600_000,
    };
    /// The health of a guardian (any living value).
    const MOB_HEALTH: u32 = 1_000;
    /// Metres within which a spawn stands where retail's did.
    const PLACE_M: f32 = 0.05;
    /// Rotation steps within which a turn is retail's.
    const TURN_STEPS: u16 = 8;
    /// Height steps within which a height is retail's.
    const HEIGHT_STEPS: u16 = 2;

    /// The test sim: the guardians stand, the encounter objects are the table's.
    struct Holdout {
        objects: EncounterObjects,
        mobs: HashMap<u64, u32>,
        next_mob: u64,
    }

    impl ScriptHost for Holdout {
        fn spawn(
            &mut self,
            _mob: &TargetMob,
            _at: [f32; 3],
            _waypoints: Option<&PatrolPath>,
        ) -> Option<u64> {
            self.next_mob += 1;
            self.mobs.insert(self.next_mob, MOB_HEALTH);
            Some(self.next_mob)
        }

        fn spawn_object(&mut self, slice: &str, at: [f32; 3], yaw_deg: f32) -> Option<u64> {
            self.objects.spawn(slice, at, yaw_deg)
        }

        fn remove(&mut self, entity: u64) {
            if EncounterObjects::is_object(entity) {
                self.objects.remove(entity);
            } else {
                self.mobs.remove(&entity);
            }
        }

        fn health(&self, entity: u64) -> Option<u32> {
            if EncounterObjects::is_object(entity) {
                return self.objects.stands(entity).then_some(OBJECT_HEALTH);
            }
            self.mobs.get(&entity).copied()
        }

        fn boss_phase(&self, _entity: u64) -> Option<u32> {
            None
        }

        fn pick(&mut self, _below: u32) -> u32 {
            0
        }
    }

    fn content_dir() -> PathBuf {
        PathBuf::from(std::env::var("NW_CONTENT_DIR").expect("NW_CONTENT_DIR"))
    }

    /// The open world encounter of slice `slice` whose root stands at `x`, `y`.
    fn open_world_encounter(dir: &Path, slice: &str, x: f32, y: f32) -> Encounter {
        let scripts = DungeonScripts::load(&dungeon_scripts_path(dir, BUILD)).expect("scripts");
        scripts
            .game_mode(OPEN_WORLD_LEVEL)
            .expect("open world")
            .encounters
            .iter()
            .find(|encounter| {
                encounter.slice == slice
                    && (encounter.position[0] - x).abs() < 1.0
                    && (encounter.position[1] - y).abs() < 1.0
            })
            .cloned()
            .expect("the retail encounter")
    }

    fn view_at(at: Position) -> ScriptView {
        ScriptView {
            living: vec![at],
            everyone: vec![at],
            present: true,
            events: Vec::new(),
        }
    }

    const fn new_slot() -> Slot {
        Slot {
            interest: 7,
            new: true,
        }
    }

    fn hex_of(group: &BundleGroup, component: u64) -> String {
        group
            .members
            .iter()
            .find(|member| member.component == component)
            .map(|member| hex::encode(member.encode().expect("encode")))
            .unwrap_or_default()
    }

    /// The root's stage members (components 5 to 10) for a new viewer.
    fn stage_states(roots: &mut EncounterRoots, run: &DungeonRun) -> Vec<String> {
        const FIRST_STAGE_COMPONENT: u64 = 5;
        const LAST_STAGE_COMPONENT: u64 = 10;
        assert!(run.client_status(0).is_some(), "status");
        let group = roots
            .group(ENCOUNTER_ROOT_ENTITY_BASE, new_slot(), &mut HashMap::new())
            .expect("root group");
        (FIRST_STAGE_COMPONENT..=LAST_STAGE_COMPONENT)
            .map(|component| hex_of(&group, component))
            .collect()
    }

    /// The standing objects of asset `asset` near `at`: (number, spawn group).
    fn objects_of(
        objects: &EncounterObjects,
        asset: &str,
        at: Position,
    ) -> Vec<(u64, BundleGroup)> {
        let mut found: Vec<(u64, BundleGroup)> = objects
            .numbers_near(at, RADIUS_M)
            .filter_map(|number| Some((number, objects.group(number, new_slot())?)))
            .filter(|(_, group)| {
                matches!(&group.members[0].state, MemberState::GdeMetadata(card)
                    if card.asset.is_some_and(|id| hex::encode(id.id) == asset))
            })
            .collect();
        found.sort_by_key(|(number, _)| *number);
        found
    }

    /// The place and the rotation's z of the position member on `component`.
    fn place_of(group: &BundleGroup, component: u64) -> (f32, f32, u16, u16) {
        let Some(MemberState::Position(PositionInTheWorldReplicatedState {
            position: Some(position),
            rotation: Some(rotation),
            ..
        })) = group
            .members
            .iter()
            .find(|member| member.component == component)
            .map(|member| &member.state)
        else {
            panic!("a position on {component}");
        };
        let RotationComponent::Value(z) = rotation.components[2] else {
            panic!("a turn about the up axis");
        };
        (position.x, position.y, position.z_raw, z)
    }

    /// Ticks `run` from `from_ms` to `to_ms`; each tick shows its statuses on the root, as the
    /// sim's encounter tick does.
    fn run_until(
        run: &mut DungeonRun,
        host: &mut Holdout,
        roots: &mut EncounterRoots,
        view: &ScriptView,
        (from_ms, to_ms): (u64, u64),
    ) {
        let mut now = from_ms;
        while now <= to_ms {
            run.tick(host, view, &[true], now);
            if let Some((manager, stages)) = run.client_status(0) {
                roots.set(0, true, manager, &stages);
            }
            now += TICK_MS;
        }
    }

    #[test]
    #[ignore = "needs the build 25810808 content files in NW_CONTENT_DIR"]
    fn the_holdout_runs_its_stages_and_spawns_its_one_way_door_as_retail() {
        let dir = content_dir();
        let encounter = open_world_encounter(&dir, HOLDOUT_SLICE, ROOT_X, ROOT_Y);
        assert_eq!(encounter.stages.len(), STAGES_INTERACT.len());
        let roots_table =
            TownProps::load_file(encounter_roots_path(&dir, BUILD)).expect("roots table");
        let objects_table =
            TownProps::load_file(encounter_objects_path(&dir, BUILD)).expect("objects table");
        let mut roots = EncounterRoots::build(&roots_table, std::slice::from_ref(&encounter));
        assert_eq!(roots.len(), 1, "the root of the holdout's slice");
        let root = roots
            .group(ENCOUNTER_ROOT_ENTITY_BASE, new_slot(), &mut HashMap::new())
            .expect("root group");
        let (x, y, _, turn) = place_of(&root, ROOT_POSITION_COMPONENT);
        assert!(
            (x - ROOT_X).abs() < PLACE_M && (y - ROOT_Y).abs() < PLACE_M,
            "{x} {y}"
        );
        assert!(turn.abs_diff(ROOT_ROTATION_Z) <= TURN_STEPS, "{turn}");
        let objects = EncounterObjects::build(&objects_table);
        for slice in [
            "slices/pois/ancient/design/holdout/worldevent_holdout_interactobject_master.dynamicslice",
            "slices/pois/ancient/design/holdout/worldevent_object_holdout_oneway_setup.dynamicslice",
        ] {
            assert!(objects.is_kind(slice), "{slice}");
        }
        let script = LevelScript {
            level: OPEN_WORLD_LEVEL.to_owned(),
            encounters: vec![encounter],
            ..LevelScript::default()
        };
        let mut run = DungeonRun::of_open_world(script, RULES, &|slice| objects.is_kind(slice));
        let mut host = Holdout {
            objects,
            mobs: HashMap::new(),
            next_mob: 0,
        };
        let door_at = Position {
            x: DOOR_X,
            y: DOOR_Y,
            z: DOOR_Z_M,
        };
        let view = view_at(door_at);
        let interact_ms = T0 + SETUP_MS + TICK_MS;
        run_until(&mut run, &mut host, &mut roots, &view, (T0, interact_ms));
        assert_eq!(
            stage_states(&mut roots, &run),
            STAGES_INTERACT,
            "Interact runs"
        );
        let glyphs = objects_of(&host.objects, GLYPH, door_at);
        assert_eq!(glyphs.len(), 1, "the glyph stands");
        assert_eq!(objects_of(&host.objects, SOLID_DOOR, door_at).len(), 1);
        assert!(objects_of(&host.objects, ONE_WAY_DOOR, door_at).is_empty());
        // The player gathers the glyph: the gathering removes it.
        host.objects.remove(glyphs[0].0);
        let holdout_ms = interact_ms + TICK_MS;
        run_until(
            &mut run,
            &mut host,
            &mut roots,
            &view,
            (holdout_ms, holdout_ms + TICK_MS),
        );
        assert_eq!(
            stage_states(&mut roots, &run),
            STAGES_HOLDOUT,
            "Holdout runs"
        );
        assert!(
            objects_of(&host.objects, SOLID_DOOR, door_at).is_empty(),
            "the solid door goes"
        );
        let doors = objects_of(&host.objects, ONE_WAY_DOOR, door_at);
        assert_eq!(doors.len(), 1, "the one-way door spawns");
        let (door, group) = &doors[0];
        assert!(*door >= ENCOUNTER_OBJECT_ENTITY_BASE);
        assert_eq!(hex_of(group, DOOR_VARIATION_COMPONENT), RETAIL_VARIATION);
        assert_eq!(hex_of(group, DOOR_GATHERABLE_COMPONENT), RETAIL_GATHERABLE);
        assert_eq!(hex_of(group, DOOR_COMPONENT), RETAIL_DOOR);
        let (x, y, z_raw, turn) = place_of(group, DOOR_POSITION_COMPONENT);
        assert!(
            (x - DOOR_X).abs() < PLACE_M && (y - DOOR_Y).abs() < PLACE_M,
            "{x} {y}"
        );
        assert!(z_raw.abs_diff(DOOR_Z_RAW) <= HEIGHT_STEPS, "{z_raw}");
        assert!(turn.abs_diff(DOOR_ROTATION_Z) <= TURN_STEPS, "{turn}");
        let anims = objects_of(&host.objects, DOOR_ANIM, door_at);
        assert_eq!(anims.len(), 1, "the door animation spawns");
        let (x, y, _, turn) = place_of(&anims[0].1, ANIM_POSITION_COMPONENT);
        assert!(
            (x - ANIM_X).abs() < PLACE_M && (y - ANIM_Y).abs() < PLACE_M,
            "{x} {y}"
        );
        assert!(turn.abs_diff(ANIM_ROTATION_Z) <= TURN_STEPS, "{turn}");
        let get_out_ms = holdout_ms + HOLDOUT_MS + TICK_MS;
        run_until(
            &mut run,
            &mut host,
            &mut roots,
            &view,
            (holdout_ms + 2 * TICK_MS, get_out_ms),
        );
        assert_eq!(
            stage_states(&mut roots, &run),
            STAGES_GET_OUT,
            "Get Out runs"
        );
        assert!(host.objects.stands(*door), "the door stays through Get Out");
        let done_ms = get_out_ms + GET_OUT_MS + CLEANUP_MS + TICK_MS;
        run_until(
            &mut run,
            &mut host,
            &mut roots,
            &view,
            (get_out_ms + TICK_MS, done_ms),
        );
        assert_eq!(
            run.client_status(0).map(|(manager, _)| manager),
            Some(STATUS_SUCCEEDED),
            "succeeded"
        );
        assert!(
            host.objects.stands(*door),
            "the door stands until the respawn"
        );
        run_until(
            &mut run,
            &mut host,
            &mut roots,
            &view,
            (done_ms + TICK_MS, done_ms + RESPAWN_MS + TICK_MS),
        );
        assert!(!host.objects.stands(*door), "the door goes at the respawn");
    }

    #[test]
    #[ignore = "needs the build 25810808 content files in NW_CONTENT_DIR"]
    fn the_stronghold_siren_shows_its_root_with_its_two_closed_doors_as_retail() {
        let dir = content_dir();
        let encounter = open_world_encounter(&dir, SIREN_SLICE, SIREN_X, SIREN_Y);
        assert_eq!(encounter.stages.len(), SIREN_STAGES);
        let roots_table =
            TownProps::load_file(encounter_roots_path(&dir, BUILD)).expect("roots table");
        let mut roots = EncounterRoots::build(&roots_table, std::slice::from_ref(&encounter));
        assert_eq!(roots.len(), 1, "the root of the siren's slice");
        let script = LevelScript {
            level: OPEN_WORLD_LEVEL.to_owned(),
            encounters: vec![encounter],
            ..LevelScript::default()
        };
        let mut run = DungeonRun::of_open_world(script, RULES, &|_| false);
        let mut host = Holdout {
            objects: EncounterObjects::default(),
            mobs: HashMap::new(),
            next_mob: 0,
        };
        let at = Position {
            x: SIREN_X,
            y: SIREN_Y,
            z: 0.0,
        };
        run_until(
            &mut run,
            &mut host,
            &mut roots,
            &view_at(at),
            (T0, T0 + TICK_MS),
        );
        let (manager, stages) = run.client_status(0).expect("status");
        assert_eq!(manager, STATUS_IN_PROGRESS, "a player near starts it");
        roots.set(0, true, manager, &stages);
        let root = roots
            .group(ENCOUNTER_ROOT_ENTITY_BASE, new_slot(), &mut HashMap::new())
            .expect("root group");
        for (component, retail) in SIREN_DOORS {
            assert_eq!(hex_of(&root, component), retail, "door on {component}");
        }
        let Some(MemberState::Position(PositionInTheWorldReplicatedState {
            position: Some(position),
            ..
        })) = root
            .members
            .iter()
            .find(|member| member.component == ROOT_POSITION_COMPONENT)
            .map(|member| &member.state)
        else {
            panic!("a position");
        };
        assert!((position.x - SIREN_X).abs() < PLACE_M, "{}", position.x);
        assert!((position.y - SIREN_Y).abs() < PLACE_M, "{}", position.y);
        assert!(
            position.z_raw.abs_diff(SIREN_Z_RAW) <= HEIGHT_STEPS,
            "{}",
            position.z_raw
        );
    }
}
