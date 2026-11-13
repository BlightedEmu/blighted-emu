// SPDX-License-Identifier: AGPL-3.0-or-later
//! The open world's dungeon entrances as network entities, and the runs each one shows
//! (dungeon entrance note, 2026-11-12; [`nw_messages::dungeon_entrance_state`]).
//!
//! Observed (117 retail ledgers): an expedition, raid or trial entrance capital is a network
//! entity that a player near it gets as a level station is got: a card of the level form (the
//! capital's id with the zero phase), the slice's other spawn states and its
//! `DungeonEntranceReplicatedState` (3105). The state lists the runs started at the entrance
//! that still run: empty at the spawn, one more element in the tick of the starter's 6776
//! `Success`, one less when the run ends. Every player who sees the entrance gets each change.
//!
//! The runs go between the sims as [`EntranceRunNews`]: the sim whose player's activation
//! opened an instance sends `Started` with the entrance and the player's group, the director
//! sends `Ended` when it ends the instance, and every sim (this one too) applies each news to
//! its entrances, so a player of a neighbour sim near the entrance sees the same list.
//!
//! The entrance of a run is the one the player opened the menu of or entered at (interaction 71
//! or 72), when it names the run's game mode; after a remote activation (5039 and 5037) it is
//! the entrance of that game mode nearest the player (Inferred: retail's remote activations
//! of the captures came near their entrance, and no capture shows one far away). A request that
//! gave the group its open instance again starts no run (Inferred: one element per instance; the
//! two members of one retail group run saw one element).
//!
//! The key of an element is the starter's group id (Observed in 9 retail group runs), the
//! starter's raid id with a zero group for a starter in a raid (Observed in 8 raid runs of the
//! gorgon raid), or zero for a player in no group (940 retail keys). The value is the run's instance id (Inferred). A sim that starts after a run
//! started does not know that run (Inferred: the news is not kept; such a run ends unseen).

use nw_bus::EntranceRunNews;
use nw_messages::dungeon_entrance_state::{
    DungeonEntranceState, NO_GROUP, NO_RAID, SPAWNED_FLAG, SpawnedDungeon,
};
use nw_messages::{BundleMember, GdeMetadataReplicatedState, MemberState};
use std::collections::HashMap;
use uuid::Uuid;

use super::crafting::{CARD_COMPONENT, Station, gde_ref_of};
use super::{Body, SimWorld};
use crate::player_content::EntranceEntity;

/// One run of an entrance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct EntranceRun {
    instance: Uuid,
    group: Option<Uuid>,
    /// The starter's raid id ([`NO_RAID`] for none).
    raid: u64,
}

/// The runs of this sim's entrances, and the news this sim sends.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct EntranceRuns {
    /// The running runs of each entrance (capital id), in their start order.
    runs: HashMap<u64, Vec<EntranceRun>>,
    /// The `Started` news of this sim's players since the last [`SimWorld::take_entrance_news`].
    outgoing: Vec<EntranceRunNews>,
}

/// The entrance entities `entrances` as level stations: the card, the slice's other spawn
/// states and the empty entrance state. An entrance whose states do not read is left out.
pub(super) fn stations(entrances: &[EntranceEntity]) -> Vec<Station> {
    entrances.iter().filter_map(station).collect()
}

fn station(entrance: &EntranceEntity) -> Option<Station> {
    let states = entrance
        .members
        .iter()
        .map(|bytes| BundleMember::read_from(bytes).map(|(member, _)| member))
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    let gde_ref = gde_ref_of(entrance.capital);
    let card = BundleMember {
        component: CARD_COMPONENT,
        state: MemberState::GdeMetadata(GdeMetadataReplicatedState {
            asset: None,
            gde_ref: Some(gde_ref),
        }),
    };
    let empty = DungeonEntranceState::default()
        .member(entrance.entrance_component)
        .ok()?;
    Some(Station {
        gde_ref,
        position: entrance.position,
        station_type: String::new(),
        name: Vec::new(),
        members: std::iter::once(card)
            .chain(states)
            .chain(std::iter::once(empty))
            .collect(),
    })
}

/// The entrance state of `runs`.
fn state_of(runs: &[EntranceRun]) -> DungeonEntranceState {
    DungeonEntranceState {
        spawned: runs
            .iter()
            .map(|run| SpawnedDungeon {
                raid: run.raid,
                group: run.group.map_or(NO_GROUP, |group| *group.as_bytes()),
                flag: SPAWNED_FLAG,
                run: *run.instance.as_bytes(),
            })
            .collect(),
    }
}

impl SimWorld {
    /// The run of instance `instance` that player `entity` started (its activation opened the
    /// instance) for game mode `crc`, at the entrance `chosen` it used when that entrance names
    /// the game mode: the `Started` news for every sim.
    pub(super) fn entrance_run_started(
        &mut self,
        entity: bevy_ecs::entity::Entity,
        crc: u32,
        chosen: Option<u64>,
        instance: Uuid,
    ) {
        let Some(entrance) = chosen
            .filter(|capital| self.entrance_mode_crc(*capital) == Some(crc))
            .or_else(|| self.nearest_entrance(entity, crc))
        else {
            return;
        };
        let Some(social) = self
            .world
            .get::<super::Player>(entity)
            .map(|player| &player.social)
        else {
            return;
        };
        // A raid member's key is its raid id alone (module doc).
        let (group, raid) = match social.raid_id() {
            Some(raid) => (None, raid),
            None => (social.group(), NO_RAID),
        };
        self.entrance_runs.outgoing.push(EntranceRunNews::Started {
            entrance,
            instance,
            group,
            raid,
        });
    }

    /// The news of this sim's run starts since the last call, for every sim.
    pub fn take_entrance_news(&mut self) -> Vec<EntranceRunNews> {
        std::mem::take(&mut self.entrance_runs.outgoing)
    }

    /// A run started or ended (from any sim or the director): the entrance's state changes for
    /// every player who sees it. A news of an entrance this sim's level does not show, a second
    /// start of one instance and the end of an unknown one change nothing.
    pub fn entrance_news(&mut self, news: EntranceRunNews) {
        let changed = match news {
            EntranceRunNews::Started {
                entrance,
                instance,
                group,
                raid,
            } => {
                if self.stations.number_of(&gde_ref_of(entrance)).is_none() {
                    return;
                }
                let runs = self.entrance_runs.runs.entry(entrance).or_default();
                if runs.iter().any(|run| run.instance == instance) {
                    return;
                }
                runs.push(EntranceRun {
                    instance,
                    group,
                    raid,
                });
                entrance
            }
            EntranceRunNews::Ended { instance } => {
                let Some((&entrance, runs)) = self
                    .entrance_runs
                    .runs
                    .iter_mut()
                    .find(|(_, runs)| runs.iter().any(|run| run.instance == instance))
                else {
                    return;
                };
                runs.retain(|run| run.instance != instance);
                entrance
            }
        };
        self.show_entrance_runs(changed);
    }

    /// Sends entrance `capital`'s runs to its viewers and keeps them in its spawn group.
    fn show_entrance_runs(&mut self, capital: u64) {
        let Some(component) = self
            .content
            .entrance_entities
            .iter()
            .find(|entrance| entrance.capital == capital)
            .map(|entrance| entrance.entrance_component)
        else {
            return;
        };
        let Some(number) = self.stations.number_of(&gde_ref_of(capital)) else {
            return;
        };
        let runs = self
            .entrance_runs
            .runs
            .get(&capital)
            .map_or(&[][..], Vec::as_slice);
        let Ok(member) = state_of(runs).member(component) else {
            return;
        };
        let kept = member.clone();
        self.stations.change_camp(
            number,
            |members| {
                for slot in members.iter_mut() {
                    if slot.component == component {
                        *slot = kept.clone();
                    }
                }
            },
            vec![member],
        );
    }

    /// The game mode CRC of the entrance whose capital is `capital`.
    fn entrance_mode_crc(&self, capital: u64) -> Option<u32> {
        let slice = self.content.entrances.get(&capital)?;
        super::instances::entrance_mode(self.content.instances.modes(), slice).map(|mode| mode.crc)
    }

    /// The entrance entity of game mode `crc` nearest player `entity`.
    fn nearest_entrance(&self, entity: bevy_ecs::entity::Entity, crc: u32) -> Option<u64> {
        let at = self.world.get::<Body>(entity)?.motion.position();
        self.content
            .entrance_entities
            .iter()
            .filter(|entrance| self.entrance_mode_crc(entrance.capital) == Some(crc))
            .min_by(|a, b| {
                a.position
                    .horizontal_distance(at)
                    .total_cmp(&b.position.horizontal_distance(at))
            })
            .map(|entrance| entrance.capital)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::slots::Slot;
    use crate::world::tests::{NOW, connection, join, tick, world_with_spawns};
    use nw_bus::InstanceReply;
    use nw_messages::{
        ArenaClientMsg, BundlePayload, FacetHeader, PLAYER_ARENA_FACET, ReplicatedStateBundle,
        ServerMessage,
    };
    use nw_reflect::Envelope;
    use nw_sim_core::Position;
    use std::sync::Arc;

    /// The entrance capital of the retail card `9b76abd0d33a810a` (`12a_msq_24_defeatgawain`
    /// 81.178 s): its id is the card's `GdeRef` read little-endian.
    const CAPITAL: u64 = 0x0a81_3ad3_d0ab_769b;
    /// Its retail spawn states: the detection volume (component 4) and the slayer script (2).
    const DETECTION_BODY: &str =
        "010f0002ff7748c62a3b0002ff7749a36e8f0002ff7748c62a3b0002ff7749a36e8f";
    const SCRIPT_BODY: &str = "01040001841c00";
    /// The component of its entrance state.
    const ENTRANCE_COMPONENT: u64 = 3;
    /// Retail `12a_msq_24_defeatgawain` 135.325 s: the run's 16 bytes.
    const RUN: u128 = 0x24d0_458d_2867_812a_bf03_ebd4_5210_7fc3;
    /// Retail `dungeon_dynasty_tank_` 256.708 s: the group of a group run.
    const GROUP: u128 = 0x51a9_6db2_d391_47fb_8160_901f_0e38_c61f;
    const PLAYER: u64 = 41;
    /// Where the player stands (x; `join` puts it at y 10, z 50).
    const PLAYER_X: f32 = 100.0;
    /// The entrance stands a few metres from it.
    const NEAR_M: f32 = 5.0;
    /// A second entrance of the same game mode, far from the player.
    const FAR_CAPITAL: u64 = 0x0a81_3ad3_d0ab_7003;
    const FAR_M: f32 = 900.0;
    /// The ticks that bring the entrance into the player's view, and their step.
    const TICKS: u64 = 10;
    const TICK_MS: u64 = 1_000;

    fn recorded(component: u64, class: u64, body: &str) -> Vec<u8> {
        BundleMember {
            component,
            state: MemberState::Recorded {
                class,
                body: hex::decode(body).expect("hex"),
            },
        }
        .encode()
        .expect("member")
    }

    fn entrance(capital: u64, x: f32) -> EntranceEntity {
        EntranceEntity {
            capital,
            position: Position {
                x,
                y: 10.0,
                z: 50.0,
            },
            entrance_component: ENTRANCE_COMPONENT,
            members: vec![
                recorded(4, 366, DETECTION_BODY),
                recorded(2, 3362, SCRIPT_BODY),
            ],
        }
    }

    /// A world with the trial's two entrances near and far from the player, and the player.
    fn world_with_entrances() -> SimWorld {
        let mode = crate::world::instances::tests::trial();
        let entrances = vec![
            entrance(CAPITAL, PLAYER_X + NEAR_M),
            entrance(FAR_CAPITAL, PLAYER_X + FAR_M),
        ];
        let slice = format!("Dungeon/Entrance/DungeonEntrance_{}", mode.id);
        let mut world = world_with_spawns(Vec::new());
        world.content = Arc::new(crate::player_content::PlayerContent {
            instances: nw_content::instances::InstanceCatalog::build([mode]),
            entrances: [(CAPITAL, slice.clone()), (FAR_CAPITAL, slice)]
                .into_iter()
                .collect(),
            entrance_entities: entrances.clone(),
            ..crate::player_content::PlayerContent::default()
        });
        world.stations.add_level(stations(&entrances));
        join(&mut world, PLAYER, PLAYER_X);
        world
    }

    fn entrance_body(world: &SimWorld, capital: u64, new: bool) -> Option<Vec<u8>> {
        let number = world.stations.number_of(&gde_ref_of(capital))?;
        let group = world.stations.group(number, Slot { interest: 1, new })?;
        group.members.iter().find_map(|member| match &member.state {
            MemberState::Recorded { class: 3105, body } => Some(body.clone()),
            _ => None,
        })
    }

    /// The bodies of the entrance states in the bundles of `PLAYER` in the ticks at `times`.
    fn sent_entrance_bodies(world: &mut SimWorld, times: &[u64]) -> Vec<String> {
        let who = connection(PLAYER);
        let mut bodies = Vec::new();
        for now in times {
            let out = tick(world, *now);
            for message in out.messages.iter().filter(|out| out.connection == who) {
                for bytes in &message.envelopes {
                    let Ok(envelope) = Envelope::decode(bytes) else {
                        continue;
                    };
                    let Some(Ok(ServerMessage::Bundle(ReplicatedStateBundle { payload, .. }))) =
                        ServerMessage::from_envelope(&envelope)
                    else {
                        continue;
                    };
                    let known = BundlePayload::decode_known(&payload).expect("payload");
                    for (member, _) in &known.members {
                        if let MemberState::Recorded { class: 3105, body } = &member.state {
                            bodies.push(hex::encode(body));
                        }
                    }
                }
            }
        }
        bodies
    }

    #[test]
    fn an_entrance_is_a_level_card_with_its_retail_states_and_an_empty_list() {
        let stations = stations(&[entrance(CAPITAL, PLAYER_X)]);
        assert_eq!(stations.len(), 1);
        let station = &stations[0];
        assert_eq!(
            hex::encode(station.gde_ref),
            "9b76abd0d33a810a0000000000000000",
            "the retail card's GdeRef"
        );
        let hex: Vec<String> = station
            .members
            .iter()
            .map(|member| hex::encode(member.encode().expect("encode")))
            .collect();
        assert_eq!(hex[0], "000a01029b76abd0d33a810a0000000000000000");
        assert_eq!(hex[1], hex::encode(recorded(4, 366, DETECTION_BODY)));
        assert_eq!(hex[2], hex::encode(recorded(2, 3362, SCRIPT_BODY)));
        assert!(hex[3].ends_with("010100"), "the retail empty list");
        assert_eq!(station.members[3].component, ENTRANCE_COMPONENT);
    }

    #[test]
    fn a_run_start_and_end_change_the_list_for_the_viewers_and_the_spawn() {
        let mut world = world_with_entrances();
        let (first, second) = (Uuid::from_u128(RUN), Uuid::from_u128(RUN + 1));
        assert_eq!(
            entrance_body(&world, CAPITAL, true)
                .map(hex::encode)
                .as_deref(),
            Some("010100")
        );
        assert_eq!(entrance_body(&world, CAPITAL, false), None, "no change yet");
        world.entrance_news(EntranceRunNews::Started {
            entrance: CAPITAL,
            instance: first,
            group: Some(Uuid::from_u128(GROUP)),
            raid: NO_RAID,
        });
        let one = "010101000000000000000051a96db2d39147fb8160901f0e38c61f0024d0458d2867812abf03ebd452107fc3";
        assert_eq!(
            entrance_body(&world, CAPITAL, false)
                .map(hex::encode)
                .as_deref(),
            Some(one),
            "the viewers get the change"
        );
        assert_eq!(
            entrance_body(&world, CAPITAL, true)
                .map(hex::encode)
                .as_deref(),
            Some(one),
            "a later viewer gets the list as it is"
        );
        // A second start of the same instance and a start at an entrance this level does not
        // show change nothing.
        world.entrance_news(EntranceRunNews::Started {
            entrance: CAPITAL,
            instance: first,
            group: None,
            raid: NO_RAID,
        });
        world.entrance_news(EntranceRunNews::Started {
            entrance: CAPITAL + 8,
            instance: second,
            group: None,
            raid: NO_RAID,
        });
        assert_eq!(
            entrance_body(&world, CAPITAL, true)
                .map(hex::encode)
                .as_deref(),
            Some(one)
        );
        world.entrance_news(EntranceRunNews::Started {
            entrance: CAPITAL,
            instance: second,
            group: None,
            raid: NO_RAID,
        });
        let body = entrance_body(&world, CAPITAL, true).expect("body");
        assert_eq!(body[2], 2, "two runs");
        world.entrance_news(EntranceRunNews::Ended { instance: first });
        assert_eq!(
            entrance_body(&world, CAPITAL, true)
                .map(hex::encode)
                .as_deref(),
            Some(
                "0101010000000000000000000000000000000000000000000000000024d0458d2867812abf03ebd452107fc4"
            ),
            "the solo run stays"
        );
        world.entrance_news(EntranceRunNews::Ended { instance: second });
        assert_eq!(
            entrance_body(&world, CAPITAL, true)
                .map(hex::encode)
                .as_deref(),
            Some("010100")
        );
        while entrance_body(&world, CAPITAL, false).is_some() {
            world.stations.tick_changes();
        }
        world.entrance_news(EntranceRunNews::Ended { instance: second });
        assert_eq!(
            entrance_body(&world, CAPITAL, false),
            None,
            "an unknown end sends nothing"
        );
    }

    #[test]
    fn the_player_near_an_entrance_gets_it_and_its_changes() {
        let mut world = world_with_entrances();
        let first_ticks: Vec<u64> = (0..TICKS).map(|tick| NOW + tick * TICK_MS).collect();
        assert_eq!(
            sent_entrance_bodies(&mut world, &first_ticks),
            ["010100"],
            "the spawn card once, with the empty list"
        );
        world.entrance_news(EntranceRunNews::Started {
            entrance: CAPITAL,
            instance: Uuid::from_u128(RUN),
            group: None,
            raid: NO_RAID,
        });
        let later = NOW + TICKS * TICK_MS;
        let bodies = sent_entrance_bodies(&mut world, &[later]);
        assert_eq!(
            bodies,
            [
                "0101010000000000000000000000000000000000000000000000000024d0458d2867812abf03ebd452107fc3"
            ],
            "the change goes out to the viewer"
        );
    }

    #[test]
    fn an_opening_activation_starts_a_run_at_the_nearest_entrance_of_its_game_mode() {
        let mut world = world_with_entrances();
        let mode = crate::world::instances::tests::trial();
        assert_eq!(world.entrance_mode_crc(CAPITAL), Some(mode.crc));
        let who = connection(PLAYER);
        let header = FacetHeader::of(PLAYER_ARENA_FACET, PLAYER);
        let activate = |world: &mut SimWorld| {
            world.arena_call(
                &who,
                ArenaClientMsg::RequestRemoteInteract {
                    header,
                    game_mode: mode.crc,
                },
                NOW,
            );
            world.arena_call(
                &who,
                ArenaClientMsg::RequestRemoteEnterDungeon { header, flag: 0 },
                NOW,
            );
            assert_eq!(world.take_instance_requests().len(), 1);
        };
        activate(&mut world);
        let instance = Uuid::from_u128(RUN);
        let grant = |opened| {
            Ok(InstanceReply::Granted {
                instance,
                sim: crate::world::tests::slot("inst-1"),
                phase: 1,
                members: Vec::new(),
                opened,
            })
        };
        world.instance_reply(&who, grant(true));
        assert_eq!(
            world.take_entrance_news(),
            [EntranceRunNews::Started {
                entrance: CAPITAL,
                instance,
                group: None,
                raid: NO_RAID,
            }],
            "the near entrance, not the far one"
        );
        assert!(world.take_entrance_news().is_empty());
    }

    /// A starter in a raid keys its run by its raid id alone (Observed: the 8 gorgon raid runs).
    #[test]
    fn a_raid_member_starts_a_run_keyed_by_its_raid() {
        const RAID: u64 = 0x14;
        let mut world = world_with_entrances();
        let mode = crate::world::instances::tests::trial();
        let who = connection(PLAYER);
        let character = world
            .player_by_number(PLAYER)
            .and_then(|entity| world.world.get::<super::super::Player>(entity))
            .map(|player| player.character)
            .expect("player");
        let state = nw_messages::social::raid_data::RaidDataState::whole(
            RAID,
            nw_messages::social::raid_data::RAID_TYPE_PLAYER,
            1,
            vec![[1; 16]; nw_messages::social::raid_data::PLAYER_RAID_GROUPS],
        );
        let mut writer = nw_messages::wire::Writer::new();
        state.write(&mut writer).expect("writes");
        world.apply_social_states(&nw_bus::SocialDeliveries {
            raid_entities: vec![nw_bus::RaidEntityDelivery {
                character,
                view: Some(nw_bus::RaidEntityView {
                    entity: [0x24; 16],
                    body: writer.finish(),
                    groups: Vec::new(),
                }),
            }],
            ..nw_bus::SocialDeliveries::default()
        });
        let header = FacetHeader::of(PLAYER_ARENA_FACET, PLAYER);
        world.arena_call(
            &who,
            ArenaClientMsg::RequestRemoteInteract {
                header,
                game_mode: mode.crc,
            },
            NOW,
        );
        world.arena_call(
            &who,
            ArenaClientMsg::RequestRemoteEnterDungeon { header, flag: 0 },
            NOW,
        );
        assert_eq!(world.take_instance_requests().len(), 1);
        let instance = Uuid::from_u128(RUN);
        world.instance_reply(
            &who,
            Ok(InstanceReply::Granted {
                instance,
                sim: crate::world::tests::slot("inst-1"),
                phase: 1,
                members: Vec::new(),
                opened: true,
            }),
        );
        let news = world.take_entrance_news();
        assert_eq!(
            news,
            [EntranceRunNews::Started {
                entrance: CAPITAL,
                instance,
                group: None,
                raid: RAID,
            }]
        );
        world.entrance_news(news[0]);
        let keyed = DungeonEntranceState {
            spawned: vec![SpawnedDungeon {
                raid: RAID,
                group: NO_GROUP,
                flag: SPAWNED_FLAG,
                run: *instance.as_bytes(),
            }],
        };
        assert_eq!(
            entrance_body(&world, CAPITAL, true),
            Some(keyed.body().expect("body")),
            "the raid id and a zero group"
        );
    }

    #[test]
    fn a_grant_of_the_groups_open_instance_starts_no_run() {
        let mut world = world_with_entrances();
        let mode = crate::world::instances::tests::trial();
        let who = connection(PLAYER);
        let header = FacetHeader::of(PLAYER_ARENA_FACET, PLAYER);
        world.arena_call(
            &who,
            ArenaClientMsg::RequestRemoteInteract {
                header,
                game_mode: mode.crc,
            },
            NOW,
        );
        world.arena_call(
            &who,
            ArenaClientMsg::RequestRemoteEnterDungeon { header, flag: 0 },
            NOW,
        );
        world.instance_reply(
            &who,
            Ok(InstanceReply::Granted {
                instance: Uuid::from_u128(RUN),
                sim: crate::world::tests::slot("inst-1"),
                phase: 1,
                members: Vec::new(),
                opened: false,
            }),
        );
        assert!(world.take_entrance_news().is_empty());
    }

    #[test]
    fn the_entrance_the_player_used_wins_over_the_nearest() {
        let mut world = world_with_entrances();
        let entity = world.player_by_number(PLAYER).expect("player");
        let crc = crate::world::instances::tests::trial().crc;
        let instance = Uuid::from_u128(RUN);
        world.entrance_run_started(entity, crc, Some(FAR_CAPITAL), instance);
        world.entrance_run_started(entity, crc + 1, Some(FAR_CAPITAL), instance);
        world.entrance_run_started(entity, crc, Some(CAPITAL + 8), instance);
        assert_eq!(
            world.take_entrance_news(),
            [
                EntranceRunNews::Started {
                    entrance: FAR_CAPITAL,
                    instance,
                    group: None,
                    raid: NO_RAID,
                },
                EntranceRunNews::Started {
                    entrance: CAPITAL,
                    instance,
                    group: None,
                    raid: NO_RAID,
                },
            ],
            "the far entrance it used; no entrance of another game mode; the nearest when the \
             one it names is no entrance"
        );
    }
}
