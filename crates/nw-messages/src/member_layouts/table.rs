// SPDX-License-Identifier: AGPL-3.0-or-later
// @generated from the client's replicated state constructors (build 25810808); see
// `super` for how. Do not edit by hand.

use super::{Child, ClassLayout, Wire};

/// The layout of each replicated state class, by type index.
pub(super) static LAYOUTS: &[ClassLayout] = &[
    ClassLayout {
        class: 12,
        name: "MB::GatherableControllerReplicatedState",
        slots: &[
            &[
                Child {
                    name: "gatherableState",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "replenishTime",
                    wire: Wire::Fixed(8),
                },
            ],
            &[],
        ],
    },
    ClassLayout {
        class: 13,
        name: "MB::PositionInTheWorldReplicatedState",
        slots: &[
            &[
                Child {
                    name: "position",
                    wire: Wire::Seq(&[Wire::Fixed(4), Wire::Fixed(4), Wire::Fixed(2)]),
                },
                Child {
                    name: "rotation",
                    wire: Wire::Quat,
                },
                Child {
                    name: "scale",
                    wire: Wire::Scale,
                },
            ],
            &[],
        ],
    },
    ClassLayout {
        class: 14,
        name: "MB::VariationComponentReplicatedState",
        slots: &[
            &[
                Child {
                    name: "Variation Index",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "Variation Index 16 Bit",
                    wire: Wire::Fixed(2),
                },
            ],
            &[],
        ],
    },
    ClassLayout {
        class: 15,
        name: "MB::VitalsComponentReplicatedState",
        slots: &[
            &[
                Child {
                    name: "HealthAmount",
                    wire: Wire::Fixed(4),
                },
                Child {
                    name: "StaminaAmount",
                    wire: Wire::Fixed(4),
                },
                Child {
                    name: "vitalsData",
                    wire: Wire::Seq(&[
                        Wire::Fixed(1),
                        Wire::Fixed(8),
                        Wire::Fixed(8),
                        Wire::Fixed(1),
                    ]),
                },
                Child {
                    name: "healthChangeFlags",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "HealthMax",
                    wire: Wire::Fixed(4),
                },
                Child {
                    name: "HealthTickRate",
                    wire: Wire::Fixed(2),
                },
                Child {
                    name: "StaminaMax",
                    wire: Wire::Fixed(4),
                },
                Child {
                    name: "StaminaTickRate",
                    wire: Wire::Fixed(2),
                },
                Child {
                    name: "vitalsId",
                    wire: Wire::Fixed(4),
                },
                Child {
                    name: "vitalsCategoryId",
                    wire: Wire::Fixed(4),
                },
                Child {
                    name: "vitalsLevel",
                    wire: Wire::Fixed(4),
                },
                Child {
                    name: "invulnerability",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "displayImmuneWhenInvulnerable",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "maxHealth",
                    wire: Wire::Fixed(2),
                },
            ],
            &[
                Child {
                    name: "ManaAmount",
                    wire: Wire::Fixed(4),
                },
                Child {
                    name: "replicatedAfflictionsHotData",
                    wire: Wire::List {
                        value: &Wire::Seq(&[
                            Wire::Fixed(2),
                            Wire::Fixed(2),
                            Wire::Seq(&[Wire::Fixed(8), Wire::Fixed(8)]),
                        ]),
                        key: &Wire::Fixed(1),
                        full: &Wire::Seq(&[
                            Wire::Fixed(1),
                            Wire::Seq(&[
                                Wire::Fixed(2),
                                Wire::Fixed(2),
                                Wire::Seq(&[Wire::Fixed(8), Wire::Fixed(8)]),
                            ]),
                        ]),
                    },
                },
                Child {
                    name: "replicatedAfflictionsColdData",
                    wire: Wire::List {
                        value: &Wire::Seq(&[
                            Wire::Fixed(2),
                            Wire::Seq(&[Wire::Fixed(1), Wire::Fixed(1)]),
                        ]),
                        key: &Wire::Fixed(1),
                        full: &Wire::Seq(&[
                            Wire::Fixed(1),
                            Wire::Seq(&[
                                Wire::Fixed(2),
                                Wire::Seq(&[Wire::Fixed(1), Wire::Fixed(1)]),
                            ]),
                        ]),
                    },
                },
                Child {
                    name: "ManaMax",
                    wire: Wire::Fixed(4),
                },
                Child {
                    name: "ManaTickRate",
                    wire: Wire::Fixed(2),
                },
            ],
            &[],
        ],
    },
    ClassLayout {
        class: 16,
        name: "MB::ProjectileReplicatedState",
        slots: &[
            &[
                Child {
                    name: "positionAnchor",
                    wire: Wire::Seq(&[Wire::Fixed(4), Wire::Fixed(4), Wire::Fixed(2)]),
                },
                Child {
                    name: "anchorHeightDelta",
                    wire: Wire::Fixed(2),
                },
                Child {
                    name: "spawnVelocity",
                    wire: Wire::Seq(&[Wire::Fixed(2), Wire::Fixed(2), Wire::Fixed(2)]),
                },
                Child {
                    name: "gravityOverride",
                    wire: Wire::Fixed(2),
                },
                Child {
                    name: "timeAnchorMicroseconds",
                    wire: Wire::Fixed(8),
                },
                Child {
                    name: "timeOffsetStartAccelMilliseconds",
                    wire: Wire::Fixed(2),
                },
                Child {
                    name: "timeOffsetBounceMilliseconds",
                    wire: Wire::Fixed(2),
                },
                Child {
                    name: "collidedWithGDEId",
                    wire: Wire::Fixed(8),
                },
                Child {
                    name: "collidedWithHitVolumeIndex",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "flags",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "owner",
                    wire: Wire::Fixed(8),
                },
                Child {
                    name: "rangedAttackIndex",
                    wire: Wire::Prefixed,
                },
                Child {
                    name: "weaponEffectCRC",
                    wire: Wire::Fixed(4),
                },
                Child {
                    name: "ammoIdCRC",
                    wire: Wire::Prefixed,
                },
                Child {
                    name: "damageTableId",
                    wire: Wire::Fixed(4),
                },
                Child {
                    name: "damageTableRowIndex",
                    wire: Wire::Fixed(2),
                },
                Child {
                    name: "m_piercingHits",
                    wire: Wire::List {
                        value: &Wire::Seq(&[Wire::Fixed(8), Wire::Fixed(1), Wire::Fixed(2)]),
                        key: &Wire::Wide,
                        full: &Wire::Seq(&[Wire::Fixed(8), Wire::Fixed(1), Wire::Fixed(2)]),
                    },
                },
                Child {
                    name: "magnetEntityId",
                    wire: Wire::Fixed(8),
                },
                Child {
                    name: "magnetRootOffset",
                    wire: Wire::Seq(&[Wire::Fixed(2), Wire::Fixed(2), Wire::Fixed(2)]),
                },
                Child {
                    name: "m_combatSettingsExpIdx",
                    wire: Wire::Fixed(1),
                },
            ],
            &[],
        ],
    },
    ClassLayout {
        class: 17,
        name: "MB::GritReplicatedState",
        slots: &[
            &[
                Child {
                    name: "cur",
                    wire: Wire::Fixed(2),
                },
                Child {
                    name: "max",
                    wire: Wire::Fixed(2),
                },
                Child {
                    name: "noHitTimeRemain",
                    wire: Wire::Fixed(2),
                },
                Child {
                    name: "multMax",
                    wire: Wire::Fixed(2),
                },
                Child {
                    name: "staggerResistMod",
                    wire: Wire::Fixed(2),
                },
                Child {
                    name: "staggerResist",
                    wire: Wire::Fixed(2),
                },
                Child {
                    name: "staggerResistNM",
                    wire: Wire::Fixed(2),
                },
                Child {
                    name: "ELSRM",
                    wire: Wire::Fixed(2),
                },
                Child {
                    name: "totalStaggerDamage",
                    wire: Wire::Fixed(2),
                },
                Child {
                    name: "last",
                    wire: Wire::Fixed(2),
                },
            ],
            &[],
        ],
    },
    ClassLayout {
        class: 28,
        name: "Javelin::RaidDataComponentReplicatedState",
        slots: &[
            &[
                Child {
                    name: "raidId",
                    wire: Wire::Fixed(8),
                },
                Child {
                    name: "raidType",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "numMembers",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "lifetime",
                    wire: Wire::Fixed(4),
                },
                Child {
                    name: "groupIds",
                    wire: Wire::Array(&Wire::Fixed(16)),
                },
            ],
            &[],
        ],
    },
    ClassLayout {
        class: 57,
        name: "MB::ProjectileSpawnerReplicatedState",
        slots: &[
            &[
                Child {
                    name: "curAmmo",
                    wire: Wire::Fixed(2),
                },
                Child {
                    name: "isFiringBlocked",
                    wire: Wire::Fixed(1),
                },
            ],
            &[],
        ],
    },
    ClassLayout {
        class: 62,
        name: "MB::LandClaimComponentReplicatedState",
        slots: &[
            &[Child {
                name: "claimKey",
                wire: Wire::Fixed(2),
            }],
            &[],
        ],
    },
    ClassLayout {
        class: 81,
        name: "MB::GatheringComponentReplicatedState",
        slots: &[
            &[],
            &[
                Child {
                    name: "m_status",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "m_amtRemaining",
                    wire: Wire::Fixed(4),
                },
                Child {
                    name: "m_gatherEfficiency",
                    wire: Wire::Fixed(4),
                },
                Child {
                    name: "m_numGathering",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "m_gatherRefCount",
                    wire: Wire::Fixed(4),
                },
                Child {
                    name: "m_waterGatherRefCount",
                    wire: Wire::Fixed(4),
                },
                Child {
                    name: "m_isGathering",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "m_gatherableGDEID",
                    wire: Wire::Fixed(8),
                },
                Child {
                    name: "m_syncGatherRefCount",
                    wire: Wire::Fixed(4),
                },
            ],
            &[],
        ],
    },
    ClassLayout {
        class: 100,
        name: "MB::PlayerNameTagComponentReplicatedState",
        slots: &[
            &[Child {
                name: "replicatedHideLevel",
                wire: Wire::Fixed(4),
            }],
            &[],
        ],
    },
    ClassLayout {
        class: 129,
        name: "MB::AttributeComponentReplicatedState",
        slots: &[
            &[
                Child {
                    name: "attributes",
                    wire: Wire::Array(&Wire::Seq(&[Wire::Fixed(4), Wire::Fixed(4)])),
                },
                Child {
                    name: "attributeBonuses",
                    wire: Wire::List {
                        value: &Wire::Fixed(4),
                        key: &Wire::Fixed(4),
                        full: &Wire::Seq(&[Wire::Fixed(4), Wire::Fixed(4)]),
                    },
                },
                Child {
                    name: "placingBonuses",
                    wire: Wire::List {
                        value: &Wire::Fixed(4),
                        key: &Wire::Wide,
                        full: &Wire::Fixed(4),
                    },
                },
                Child {
                    name: "persistentAttributeData",
                    wire: Wire::Seq(&[
                        Wire::Fixed(4),
                        Wire::Fixed(1),
                        Wire::Fixed(1),
                        Wire::Array(&Wire::Seq(&[Wire::Fixed(4), Wire::Fixed(4)])),
                        Wire::Fixed(4),
                    ]),
                },
            ],
            &[],
        ],
    },
    ClassLayout {
        class: 185,
        name: "MB::AbilityComponentReplicatedState",
        slots: &[
            &[
                Child {
                    name: "persistentAbilityData",
                    wire: Wire::Seq(&[
                        Wire::Array(&Wire::Seq(&[
                            Wire::Fixed(4),
                            Wire::Array(&Wire::Seq(&[Wire::Fixed(4), Wire::Fixed(4)])),
                        ])),
                        Wire::Array(&Wire::Seq(&[Wire::Fixed(4), Wire::Fixed(4)])),
                    ]),
                },
                Child {
                    name: "hitDataNumHits",
                    wire: Wire::List {
                        value: &Wire::Fixed(1),
                        key: &Wire::Wide,
                        full: &Wire::Fixed(1),
                    },
                },
                Child {
                    name: "hitDataAbilityIds",
                    wire: Wire::List {
                        value: &Wire::Fixed(4),
                        key: &Wire::Wide,
                        full: &Wire::Fixed(4),
                    },
                },
                Child {
                    name: "actionDataCount",
                    wire: Wire::List {
                        value: &Wire::Fixed(1),
                        key: &Wire::Wide,
                        full: &Wire::Fixed(1),
                    },
                },
                Child {
                    name: "actionDataAbilityIds",
                    wire: Wire::List {
                        value: &Wire::Fixed(4),
                        key: &Wire::Wide,
                        full: &Wire::Fixed(4),
                    },
                },
            ],
            &[],
        ],
    },
    ClassLayout {
        class: 205,
        name: "MB::CraftingComponentReplicatedState",
        slots: &[
            &[
                Child {
                    name: "m_cooldowns",
                    wire: Wire::List {
                        value: &Wire::Seq(&[Wire::Fixed(1), Wire::Fixed(8)]),
                        key: &Wire::Fixed(4),
                        full: &Wire::Seq(&[Wire::Fixed(4), Wire::Fixed(1), Wire::Fixed(8)]),
                    },
                },
                Child {
                    name: "m_craftGSBonuses",
                    wire: Wire::List {
                        value: &Wire::Fixed(2),
                        key: &Wire::Fixed(1),
                        full: &Wire::Seq(&[Wire::Fixed(1), Wire::Fixed(2)]),
                    },
                },
            ],
            &[],
        ],
    },
    ClassLayout {
        class: 333,
        name: "Javelin::CapturePointReplicatedState",
        slots: &[
            &[
                Child {
                    name: "coldData",
                    wire: Wire::Seq(&[
                        Wire::Seq(&[Wire::Fixed(4), Wire::Fixed(4), Wire::Fixed(4)]),
                        Wire::Fixed(4),
                        Wire::Fixed(1),
                        Wire::Seq(&[Wire::Fixed(8), Wire::Fixed(1)]),
                    ]),
                },
                Child {
                    name: "hotData",
                    wire: Wire::Fixed(1),
                },
            ],
            &[],
        ],
    },
    ClassLayout {
        class: 363,
        name: "MB::EventTimelineComponentReplicatedState",
        slots: &[
            &[
                Child {
                    name: "m_timelineIndex",
                    wire: Wire::Fixed(2),
                },
                Child {
                    name: "m_timelineStatus",
                    wire: Wire::Fixed(1),
                },
            ],
            &[],
        ],
    },
    ClassLayout {
        class: 366,
        name: "MB::DetectionVolumeEventReplicatedState",
        slots: &[
            &[
                Child {
                    name: "lastOnEnterEventsExecutionTime",
                    wire: Wire::Fixed(8),
                },
                Child {
                    name: "lastOnExitEventsExecutionTime",
                    wire: Wire::Fixed(8),
                },
                Child {
                    name: "lastOnFirstEnterEventsExecutionTime",
                    wire: Wire::Fixed(8),
                },
                Child {
                    name: "lastOnLastExitEventsExecutionTime",
                    wire: Wire::Fixed(8),
                },
            ],
            &[],
        ],
    },
    ClassLayout {
        class: 403,
        name: "Javelin::BossPhaseComponentReplicatedState",
        slots: &[
            &[
                Child {
                    name: "m_bossPhase",
                    wire: Wire::Fixed(4),
                },
                Child {
                    name: "m_spawnedInEncounter",
                    wire: Wire::Fixed(1),
                },
            ],
            &[],
        ],
    },
    ClassLayout {
        class: 497,
        name: "Javelin::GameEventComponentReplicatedState",
        slots: &[
            &[
                Child {
                    name: "gameEvents",
                    wire: Wire::List {
                        value: &Wire::Seq(&[
                            Wire::Fixed(8),
                            Wire::Seq(&[
                                Wire::Fixed(4),
                                Wire::Fixed(4),
                                Wire::Fixed(4),
                                Wire::Fixed(4),
                                Wire::Fixed(1),
                                Wire::Fixed(4),
                                Wire::Fixed(4),
                                Wire::Seq(&[
                                    Wire::Fixed(4),
                                    Wire::Array(&Wire::Seq(&[
                                        Wire::Fixed(4),
                                        Wire::Fixed(2),
                                        Wire::Seq(&[
                                            Wire::Fixed(4),
                                            Wire::Fixed(1),
                                            Wire::Fixed(4),
                                            Wire::Fixed(4),
                                            Wire::Fixed(1),
                                            Wire::Fixed(1),
                                            Wire::Seq(&[
                                                Wire::Array(&Wire::Fixed(4)),
                                                Wire::Array(&Wire::Fixed(4)),
                                                Wire::Fixed(1),
                                                Wire::Array(&Wire::Fixed(4)),
                                            ]),
                                        ]),
                                    ])),
                                    Wire::Fixed(1),
                                    Wire::Fixed(1),
                                    Wire::Fixed(4),
                                    Wire::Fixed(2),
                                    Wire::Fixed(4),
                                    Wire::Fixed(4),
                                ]),
                            ]),
                        ]),
                        key: &Wire::Wide,
                        full: &Wire::Seq(&[
                            Wire::Fixed(8),
                            Wire::Seq(&[
                                Wire::Fixed(4),
                                Wire::Fixed(4),
                                Wire::Fixed(4),
                                Wire::Fixed(4),
                                Wire::Fixed(1),
                                Wire::Fixed(4),
                                Wire::Fixed(4),
                                Wire::Seq(&[
                                    Wire::Fixed(4),
                                    Wire::Array(&Wire::Seq(&[
                                        Wire::Fixed(4),
                                        Wire::Fixed(2),
                                        Wire::Seq(&[
                                            Wire::Fixed(4),
                                            Wire::Fixed(1),
                                            Wire::Fixed(4),
                                            Wire::Fixed(4),
                                            Wire::Fixed(1),
                                            Wire::Fixed(1),
                                            Wire::Seq(&[
                                                Wire::Array(&Wire::Fixed(4)),
                                                Wire::Array(&Wire::Fixed(4)),
                                                Wire::Fixed(1),
                                                Wire::Array(&Wire::Fixed(4)),
                                            ]),
                                        ]),
                                    ])),
                                    Wire::Fixed(1),
                                    Wire::Fixed(1),
                                    Wire::Fixed(4),
                                    Wire::Fixed(2),
                                    Wire::Fixed(4),
                                    Wire::Fixed(4),
                                ]),
                            ]),
                        ]),
                    },
                },
                Child {
                    name: "dailyBonusesUsed",
                    wire: Wire::List {
                        value: &Wire::Seq(&[Wire::Fixed(4), Wire::Fixed(1)]),
                        key: &Wire::Wide,
                        full: &Wire::Seq(&[Wire::Fixed(4), Wire::Fixed(1)]),
                    },
                },
            ],
            &[],
        ],
    },
    ClassLayout {
        class: 603,
        name: "MB::AudioProxyComponentReplicatedState",
        slots: &[
            &[Child {
                name: "scriptListForJoints",
                wire: Wire::Array(&Wire::Fixed(4)),
            }],
            &[],
        ],
    },
    ClassLayout {
        class: 610,
        name: "MB::TwitchStreamReplicatedState",
        slots: &[
            &[
                Child {
                    name: "isLive",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "numViewers",
                    wire: Wire::Fixed(4),
                },
                Child {
                    name: "channelName",
                    wire: Wire::Text,
                },
            ],
            &[],
        ],
    },
    ClassLayout {
        class: 670,
        name: "MB::IncapacitatedReplicatedState",
        slots: &[
            &[Child {
                name: "enabled",
                wire: Wire::Fixed(1),
            }],
            &[],
        ],
    },
    ClassLayout {
        class: 899,
        name: "MB::ProgressionComponentReplicatedState",
        slots: &[
            &[
                Child {
                    name: "level",
                    wire: Wire::Fixed(4),
                },
                Child {
                    name: "bonusLevel",
                    wire: Wire::Fixed(4),
                },
            ],
            &[
                Child {
                    name: "experiencePoints",
                    wire: Wire::Fixed(4),
                },
                Child {
                    name: "restedExp",
                    wire: Wire::Fixed(4),
                },
            ],
            &[],
        ],
    },
    ClassLayout {
        class: 911,
        name: "MB::CategoricalProgressionReplicatedState",
        slots: &[
            &[
                Child {
                    name: "progressionIds",
                    wire: Wire::List {
                        value: &Wire::Fixed(4),
                        key: &Wire::Wide,
                        full: &Wire::Fixed(4),
                    },
                },
                Child {
                    name: "ranks",
                    wire: Wire::List {
                        value: &Wire::Fixed(2),
                        key: &Wire::Wide,
                        full: &Wire::Fixed(2),
                    },
                },
                Child {
                    name: "points",
                    wire: Wire::List {
                        value: &Wire::Fixed(8),
                        key: &Wire::Wide,
                        full: &Wire::Fixed(8),
                    },
                },
            ],
            &[],
        ],
    },
    ClassLayout {
        class: 982,
        name: "MB::LootTrackerComponentReplicatedState",
        slots: &[
            &[
                Child {
                    name: "m_lootDataMap",
                    wire: Wire::List {
                        value: &Wire::Seq(&[
                            Wire::Fixed(4),
                            Wire::Seq(&[
                                Wire::Fixed(4),
                                Wire::Fixed(8),
                                Wire::Fixed(8),
                                Wire::Fixed(1),
                                Wire::Fixed(12),
                                Wire::Fixed(4),
                                Wire::Fixed(1),
                            ]),
                        ]),
                        key: &Wire::Fixed(16),
                        full: &Wire::Seq(&[
                            Wire::Fixed(16),
                            Wire::Fixed(4),
                            Wire::Seq(&[
                                Wire::Fixed(4),
                                Wire::Fixed(8),
                                Wire::Fixed(8),
                                Wire::Fixed(1),
                                Wire::Fixed(12),
                                Wire::Fixed(4),
                                Wire::Fixed(1),
                            ]),
                        ]),
                    },
                },
                Child {
                    name: "m_lootCollectibles",
                    wire: Wire::List {
                        value: &Wire::Fixed(8),
                        key: &Wire::Wide,
                        full: &Wire::Fixed(8),
                    },
                },
                Child {
                    name: "m_failedRollBonusPercent",
                    wire: Wire::Fixed(4),
                },
                Child {
                    name: "m_slayerScriptDataMap",
                    wire: Wire::List {
                        value: &Wire::Seq(&[Wire::Fixed(1), Wire::Fixed(8)]),
                        key: &Wire::Fixed(16),
                        full: &Wire::Seq(&[Wire::Fixed(16), Wire::Fixed(1), Wire::Fixed(8)]),
                    },
                },
                Child {
                    name: "m_lootDivertMap",
                    wire: Wire::List {
                        value: &Wire::Seq(&[
                            Wire::Fixed(1),
                            Wire::Fixed(8),
                            Wire::Fixed(2),
                            Wire::Fixed(1),
                        ]),
                        key: &Wire::Fixed(4),
                        full: &Wire::Seq(&[
                            Wire::Fixed(4),
                            Wire::Seq(&[
                                Wire::Fixed(1),
                                Wire::Fixed(8),
                                Wire::Fixed(2),
                                Wire::Fixed(1),
                            ]),
                        ]),
                    },
                },
                Child {
                    name: "m_lootLimitDataMap",
                    wire: Wire::List {
                        value: &Wire::Seq(&[
                            Wire::Fixed(8),
                            Wire::Fixed(8),
                            Wire::Fixed(2),
                            Wire::Fixed(1),
                        ]),
                        key: &Wire::Fixed(4),
                        full: &Wire::Seq(&[
                            Wire::Fixed(4),
                            Wire::Seq(&[
                                Wire::Fixed(8),
                                Wire::Fixed(8),
                                Wire::Fixed(2),
                                Wire::Fixed(1),
                            ]),
                        ]),
                    },
                },
            ],
            &[],
        ],
    },
    ClassLayout {
        class: 1055,
        name: "MB::FtueDetectionVolumeTeleportReplicatedState",
        slots: &[
            &[Child {
                name: "playerEnteredTrigger",
                wire: Wire::Fixed(1),
            }],
            &[],
        ],
    },
    ClassLayout {
        class: 1091,
        name: "Javelin::GroupFinderGroupDataComponentReplicatedState",
        slots: &[
            &[],
            &[
                Child {
                    name: "groupFinderGroupId",
                    wire: Wire::Fixed(16),
                },
                Child {
                    name: "groupMetadata",
                    wire: Wire::Seq(&[
                        Wire::Fixed(1),
                        Wire::Fixed(1),
                        Wire::Fixed(1),
                        Wire::Fixed(4),
                        Wire::Fixed(4),
                        Wire::Seq(&[
                            Wire::Fixed(4),
                            Wire::Fixed(4),
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                            Wire::Array(&Wire::Fixed(1)),
                            Wire::Fixed(8),
                        ]),
                    ]),
                },
                Child {
                    name: "memberBlobs",
                    wire: Wire::List {
                        value: &Wire::Seq(&[
                            Wire::Text,
                            Wire::Seq(&[Wire::Fixed(4), Wire::Text]),
                            Wire::Fixed(1),
                            Wire::Fixed(4),
                            Wire::Fixed(4),
                            Wire::Seq(&[
                                Wire::Fixed(4),
                                Wire::Fixed(4),
                                Wire::Fixed(4),
                                Wire::Fixed(4),
                                Wire::Fixed(4),
                                Wire::Fixed(2),
                                Wire::Fixed(2),
                                Wire::Fixed(4),
                                Wire::Fixed(4),
                                Wire::Fixed(1),
                                Wire::Fixed(1),
                                Wire::Fixed(1),
                                Wire::Seq(&[Wire::Fixed(1), Wire::Fixed(1)]),
                            ]),
                        ]),
                        key: &Wire::Wide,
                        full: &Wire::Seq(&[
                            Wire::Text,
                            Wire::Seq(&[Wire::Fixed(4), Wire::Text]),
                            Wire::Fixed(1),
                            Wire::Fixed(4),
                            Wire::Fixed(4),
                            Wire::Seq(&[
                                Wire::Fixed(4),
                                Wire::Fixed(4),
                                Wire::Fixed(4),
                                Wire::Fixed(4),
                                Wire::Fixed(4),
                                Wire::Fixed(2),
                                Wire::Fixed(2),
                                Wire::Fixed(4),
                                Wire::Fixed(4),
                                Wire::Fixed(1),
                                Wire::Fixed(1),
                                Wire::Fixed(1),
                                Wire::Seq(&[Wire::Fixed(1), Wire::Fixed(1)]),
                            ]),
                        ]),
                    },
                },
                Child {
                    name: "isPendingConversion",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "pendingConversionConfirmations",
                    wire: Wire::Fixed(1),
                },
            ],
            &[Child {
                name: "hasPendingApplications",
                wire: Wire::Fixed(1),
            }],
            &[
                Child {
                    name: "applicationIds",
                    wire: Wire::List {
                        value: &Wire::Text,
                        key: &Wire::Wide,
                        full: &Wire::Text,
                    },
                },
                Child {
                    name: "applicationMemberBlobs",
                    wire: Wire::List {
                        value: &Wire::Seq(&[
                            Wire::Text,
                            Wire::Seq(&[Wire::Fixed(4), Wire::Text]),
                            Wire::Fixed(1),
                            Wire::Fixed(4),
                            Wire::Fixed(4),
                            Wire::Seq(&[
                                Wire::Fixed(4),
                                Wire::Fixed(4),
                                Wire::Fixed(4),
                                Wire::Fixed(4),
                                Wire::Fixed(4),
                                Wire::Fixed(2),
                                Wire::Fixed(2),
                                Wire::Fixed(4),
                                Wire::Fixed(4),
                                Wire::Fixed(1),
                                Wire::Fixed(1),
                                Wire::Fixed(1),
                                Wire::Seq(&[Wire::Fixed(1), Wire::Fixed(1)]),
                            ]),
                        ]),
                        key: &Wire::Wide,
                        full: &Wire::Seq(&[
                            Wire::Text,
                            Wire::Seq(&[Wire::Fixed(4), Wire::Text]),
                            Wire::Fixed(1),
                            Wire::Fixed(4),
                            Wire::Fixed(4),
                            Wire::Seq(&[
                                Wire::Fixed(4),
                                Wire::Fixed(4),
                                Wire::Fixed(4),
                                Wire::Fixed(4),
                                Wire::Fixed(4),
                                Wire::Fixed(2),
                                Wire::Fixed(2),
                                Wire::Fixed(4),
                                Wire::Fixed(4),
                                Wire::Fixed(1),
                                Wire::Fixed(1),
                                Wire::Fixed(1),
                                Wire::Seq(&[Wire::Fixed(1), Wire::Fixed(1)]),
                            ]),
                        ]),
                    },
                },
                Child {
                    name: "applicationStates",
                    wire: Wire::List {
                        value: &Wire::Fixed(1),
                        key: &Wire::Wide,
                        full: &Wire::Fixed(1),
                    },
                },
            ],
            &[],
        ],
    },
    ClassLayout {
        class: 1195,
        name: "MB::PlayerAppearanceComponentReplicatedState",
        slots: &[
            &[
                Child {
                    name: "playerGender",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "playerRace",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "playerSkinTone",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "playerHairstyle",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "playerFacialHair",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "playerHairColor",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "playerFacialHairColor",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "playerEyeColor",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "playerFaceMark",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "playerScar",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "playerTattoo",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "playerTattooColor",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "iconData",
                    wire: Wire::Seq(&[
                        Wire::Fixed(1),
                        Wire::Fixed(1),
                        Wire::Fixed(1),
                        Wire::Fixed(1),
                        Wire::Fixed(1),
                        Wire::Fixed(1),
                        Wire::Fixed(1),
                        Wire::Fixed(2),
                    ]),
                },
                Child {
                    name: "appearanceChangeFlag",
                    wire: Wire::Fixed(1),
                },
            ],
            &[],
        ],
    },
    ClassLayout {
        class: 1292,
        name: "MB::CampingComponentReplicatedState",
        slots: &[
            &[
                Child {
                    name: "currentCampRef",
                    wire: Wire::Fixed(24),
                },
                Child {
                    name: "currentCampSkinId",
                    wire: Wire::Fixed(4),
                },
                Child {
                    name: "isCampUnderAttack",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "campBindCooldownEndTime",
                    wire: Wire::Fixed(8),
                },
            ],
            &[],
        ],
    },
    ClassLayout {
        class: 1296,
        name: "Javelin::ArenaReplicatedState",
        slots: &[
            &[
                Child {
                    name: "isActive",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "currentStageEntityId",
                    wire: Wire::Fixed(8),
                },
                Child {
                    name: "currentStageStartTime",
                    wire: Wire::Fixed(8),
                },
            ],
            &[],
        ],
    },
    ClassLayout {
        class: 1525,
        name: "MB::StatMultiplierTableComponentReplicatedState",
        slots: &[
            &[],
            &[
                Child {
                    name: "multiplierTable",
                    wire: Wire::List {
                        value: &Wire::Seq(&[Wire::Fixed(4), Wire::Fixed(1)]),
                        key: &Wire::Fixed(1),
                        full: &Wire::Seq(&[
                            Wire::Fixed(1),
                            Wire::Seq(&[Wire::Fixed(4), Wire::Fixed(1)]),
                        ]),
                    },
                },
                Child {
                    name: "staminaCostReductionMultipliers",
                    wire: Wire::List {
                        value: &Wire::Fixed(4),
                        key: &Wire::Fixed(4),
                        full: &Wire::Seq(&[Wire::Fixed(4), Wire::Fixed(4)]),
                    },
                },
                Child {
                    name: "xpIncreaseMultipliers",
                    wire: Wire::List {
                        value: &Wire::Fixed(4),
                        key: &Wire::Fixed(4),
                        full: &Wire::Seq(&[Wire::Fixed(4), Wire::Fixed(4)]),
                    },
                },
            ],
            &[Child {
                name: "remoteMultiplierTable",
                wire: Wire::List {
                    value: &Wire::Seq(&[Wire::Fixed(4), Wire::Fixed(1)]),
                    key: &Wire::Fixed(1),
                    full: &Wire::Seq(&[
                        Wire::Fixed(1),
                        Wire::Seq(&[Wire::Fixed(4), Wire::Fixed(1)]),
                    ]),
                },
            }],
            &[],
        ],
    },
    ClassLayout {
        class: 1528,
        name: "MB::DamageReceiverComponentReplicatedState",
        slots: &[
            &[
                Child {
                    name: "isBlockActive",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "blockWeaponSlotAlias",
                    wire: Wire::Fixed(4),
                },
                Child {
                    name: "debugCritChance",
                    wire: Wire::Fixed(4),
                },
            ],
            &[],
        ],
    },
    ClassLayout {
        class: 1566,
        name: "MB::ChatReplicatedState",
        slots: &[
            &[Child {
                name: "chatMutes",
                wire: Wire::List {
                    value: &Wire::Text,
                    key: &Wire::Wide,
                    full: &Wire::Text,
                },
            }],
            &[],
        ],
    },
    ClassLayout {
        class: 1594,
        name: "MB::BuildableControllerReplicatedState",
        slots: &[
            &[
                Child {
                    name: "currentState",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "checkFoundationEvents",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "commitedResources",
                    wire: Wire::List {
                        value: &Wire::Seq(&[
                            Wire::Fixed(4),
                            Wire::Fixed(2),
                            Wire::Seq(&[
                                Wire::Fixed(4),
                                Wire::Fixed(1),
                                Wire::Fixed(4),
                                Wire::Fixed(4),
                                Wire::Fixed(1),
                                Wire::Fixed(1),
                                Wire::Seq(&[
                                    Wire::Array(&Wire::Fixed(4)),
                                    Wire::Array(&Wire::Fixed(4)),
                                    Wire::Fixed(1),
                                    Wire::Array(&Wire::Fixed(4)),
                                ]),
                            ]),
                        ]),
                        key: &Wire::Text,
                        full: &Wire::Seq(&[
                            Wire::Text,
                            Wire::Seq(&[
                                Wire::Fixed(4),
                                Wire::Fixed(2),
                                Wire::Seq(&[
                                    Wire::Fixed(4),
                                    Wire::Fixed(1),
                                    Wire::Fixed(4),
                                    Wire::Fixed(4),
                                    Wire::Fixed(1),
                                    Wire::Fixed(1),
                                    Wire::Seq(&[
                                        Wire::Array(&Wire::Fixed(4)),
                                        Wire::Array(&Wire::Fixed(4)),
                                        Wire::Fixed(1),
                                        Wire::Array(&Wire::Fixed(4)),
                                    ]),
                                ]),
                            ]),
                        ]),
                    },
                },
                Child {
                    name: "ruinDamageType",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "territoryOwnerGuildId",
                    wire: Wire::Fixed(16),
                },
                Child {
                    name: "upgradeTimePoint",
                    wire: Wire::Fixed(8),
                },
                Child {
                    name: "craftingStationTypeCrc",
                    wire: Wire::Fixed(4),
                },
                Child {
                    name: "campTierBuildableId",
                    wire: Wire::Fixed(4),
                },
                Child {
                    name: "isPvPCamp",
                    wire: Wire::Fixed(1),
                },
            ],
            &[],
        ],
    },
    ClassLayout {
        class: 1647,
        name: "MB::TimelineComponentReplicatedState",
        slots: &[
            &[
                Child {
                    name: "PlayStates",
                    wire: Wire::List {
                        value: &Wire::Fixed(1),
                        key: &Wire::Wide,
                        full: &Wire::Fixed(1),
                    },
                },
                Child {
                    name: "PlayHeads",
                    wire: Wire::List {
                        value: &Wire::Fixed(4),
                        key: &Wire::Wide,
                        full: &Wire::Fixed(4),
                    },
                },
            ],
            &[],
        ],
    },
    ClassLayout {
        class: 1739,
        name: "MB::WarDataComponentReplicatedState",
        slots: &[
            &[
                Child {
                    name: "m_warData",
                    wire: Wire::List {
                        value: &Wire::Seq(&[
                            Wire::Fixed(16),
                            Wire::Fixed(16),
                            Wire::Fixed(8),
                            Wire::Array(&Wire::Fixed(16)),
                            Wire::Fixed(16),
                            Wire::Fixed(8),
                            Wire::Seq(&[
                                Wire::Fixed(8),
                                Wire::Array(&Wire::Fixed(16)),
                                Wire::Fixed(16),
                                Wire::Fixed(8),
                                Wire::Fixed(1),
                                Wire::Fixed(1),
                                Wire::Fixed(2),
                                Wire::Fixed(4),
                                Wire::Fixed(16),
                                Wire::Fixed(8),
                                Wire::Seq(&[
                                    Wire::Fixed(16),
                                    Wire::Fixed(1),
                                    Wire::Fixed(4),
                                    Wire::Fixed(1),
                                    Wire::Fixed(1),
                                    Wire::Fixed(4),
                                    Wire::Fixed(8),
                                    Wire::Seq(&[Wire::Fixed(4), Wire::Fixed(16), Wire::Fixed(16)]),
                                ]),
                            ]),
                        ]),
                        key: &Wire::Wide,
                        full: &Wire::Seq(&[
                            Wire::Fixed(16),
                            Wire::Fixed(16),
                            Wire::Fixed(8),
                            Wire::Array(&Wire::Fixed(16)),
                            Wire::Fixed(16),
                            Wire::Fixed(8),
                            Wire::Seq(&[
                                Wire::Fixed(8),
                                Wire::Array(&Wire::Fixed(16)),
                                Wire::Fixed(16),
                                Wire::Fixed(8),
                                Wire::Fixed(1),
                                Wire::Fixed(1),
                                Wire::Fixed(2),
                                Wire::Fixed(4),
                                Wire::Fixed(16),
                                Wire::Fixed(8),
                                Wire::Seq(&[
                                    Wire::Fixed(16),
                                    Wire::Fixed(1),
                                    Wire::Fixed(4),
                                    Wire::Fixed(1),
                                    Wire::Fixed(1),
                                    Wire::Fixed(4),
                                    Wire::Fixed(8),
                                    Wire::Seq(&[Wire::Fixed(4), Wire::Fixed(16), Wire::Fixed(16)]),
                                ]),
                            ]),
                        ]),
                    },
                },
                Child {
                    name: "m_warScheduleAdjustments",
                    wire: Wire::List {
                        value: &Wire::Seq(&[
                            Wire::Fixed(16),
                            Wire::Fixed(4),
                            Wire::Fixed(2),
                            Wire::Fixed(4),
                            Wire::Fixed(8),
                        ]),
                        key: &Wire::Fixed(2),
                        full: &Wire::Seq(&[
                            Wire::Fixed(2),
                            Wire::Seq(&[
                                Wire::Fixed(16),
                                Wire::Fixed(4),
                                Wire::Fixed(2),
                                Wire::Fixed(4),
                                Wire::Fixed(8),
                            ]),
                        ]),
                    },
                },
                Child {
                    name: "m_influenceRaceData",
                    wire: Wire::List {
                        value: &Wire::Seq(&[
                            Wire::Fixed(8),
                            Wire::Fixed(2),
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                            Wire::Array(&Wire::Fixed(1)),
                        ]),
                        key: &Wire::Fixed(2),
                        full: &Wire::Seq(&[
                            Wire::Fixed(2),
                            Wire::Seq(&[
                                Wire::Fixed(8),
                                Wire::Fixed(2),
                                Wire::Fixed(1),
                                Wire::Fixed(1),
                                Wire::Fixed(1),
                                Wire::Fixed(1),
                                Wire::Array(&Wire::Fixed(1)),
                            ]),
                        ]),
                    },
                },
            ],
            &[],
        ],
    },
    ClassLayout {
        class: 1742,
        name: "MB::AggregateContractCountComponentReplicatedState",
        slots: &[
            &[
                Child {
                    name: "replicatedOutpostActorIdMap",
                    wire: Wire::List {
                        value: &Wire::Text,
                        key: &Wire::Text,
                        full: &Wire::Seq(&[Wire::Text, Wire::Text]),
                    },
                },
                Child {
                    name: "replicatedOutpostIdVector",
                    wire: Wire::List {
                        value: &Wire::Text,
                        key: &Wire::Wide,
                        full: &Wire::Text,
                    },
                },
                Child {
                    name: "replicatedOutpostContractCountsVector",
                    wire: Wire::List {
                        value: &Wire::Fixed(4),
                        key: &Wire::Wide,
                        full: &Wire::Fixed(4),
                    },
                },
            ],
            &[],
        ],
    },
    ClassLayout {
        class: 1746,
        name: "MB::TerritoryInterfaceComponentReplicatedState",
        slots: &[
            &[
                Child {
                    name: "territoryFunctionalityDown",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "territoryRefsSet",
                    wire: Wire::Fixed(1),
                },
            ],
            &[Child {
                name: "territoryProgressionData",
                wire: Wire::Seq(&[
                    Wire::Array(&Wire::Seq(&[
                        Wire::Fixed(1),
                        Wire::Seq(&[Wire::Fixed(4), Wire::Fixed(8), Wire::Fixed(4)]),
                    ])),
                    Wire::Array(&Wire::Seq(&[Wire::Fixed(4), Wire::Fixed(8)])),
                    Wire::Seq(&[Wire::Fixed(4), Wire::Fixed(8), Wire::Fixed(4)]),
                    Wire::Array(&Wire::Seq(&[Wire::Fixed(4), Wire::Fixed(8)])),
                ]),
            }],
            &[Child {
                name: "territoryGovernanceData",
                wire: Wire::Seq(&[
                    Wire::Seq(&[
                        Wire::Fixed(8),
                        Wire::Fixed(8),
                        Wire::Fixed(8),
                        Wire::Array(&Wire::Seq(&[Wire::Fixed(1), Wire::Fixed(8)])),
                        Wire::Seq(&[
                            Wire::Seq(&[
                                Wire::Fixed(8),
                                Wire::Array(&Wire::Seq(&[
                                    Wire::Fixed(1),
                                    Wire::Seq(&[Wire::Fixed(8), Wire::Fixed(8), Wire::Fixed(8)]),
                                ])),
                                Wire::Fixed(8),
                            ]),
                            Wire::Array(&Wire::Seq(&[
                                Wire::Fixed(8),
                                Wire::Array(&Wire::Seq(&[
                                    Wire::Fixed(1),
                                    Wire::Seq(&[Wire::Fixed(8), Wire::Fixed(8), Wire::Fixed(8)]),
                                ])),
                                Wire::Fixed(8),
                            ])),
                        ]),
                    ]),
                    Wire::Fixed(4),
                ]),
            }],
            &[],
        ],
    },
    ClassLayout {
        class: 1755,
        name: "MB::ContainerComponentReplicatedState",
        slots: &[
            &[
                Child {
                    name: "Container",
                    wire: Wire::List {
                        value: &Wire::Seq(&[
                            Wire::Fixed(8),
                            Wire::Prefixed,
                            Wire::Prefixed,
                            Wire::Prefixed,
                            Wire::Prefixed,
                            Wire::Prefixed,
                            Wire::Prefixed,
                            Wire::Prefixed,
                            Wire::Prefixed,
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                            Wire::Fixed(16),
                            Wire::Fixed(1),
                            Wire::Fixed(4),
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                        ]),
                        key: &Wire::Wide,
                        full: &Wire::Seq(&[
                            Wire::Fixed(8),
                            Wire::Prefixed,
                            Wire::Prefixed,
                            Wire::Prefixed,
                            Wire::Prefixed,
                            Wire::Prefixed,
                            Wire::Prefixed,
                            Wire::Prefixed,
                            Wire::Prefixed,
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                            Wire::Fixed(16),
                            Wire::Fixed(1),
                            Wire::Fixed(4),
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                        ]),
                    },
                },
                Child {
                    name: "Item Class",
                    wire: Wire::Array(&Wire::Fixed(8)),
                },
                Child {
                    name: "Bonus Max Encumbrance",
                    wire: Wire::Fixed(4),
                },
                Child {
                    name: "Can Transfer items",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "Container Was Emptied",
                    wire: Wire::Fixed(1),
                },
            ],
            &[],
        ],
    },
    ClassLayout {
        class: 1880,
        name: "MB::TerritoryInteractorReplicatedState",
        slots: &[
            &[
                Child {
                    name: "replicatedPlayerSignupDataMap",
                    wire: Wire::List {
                        value: &Wire::Seq(&[
                            Wire::Fixed(16),
                            Wire::Fixed(4),
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                        ]),
                        key: &Wire::Fixed(2),
                        full: &Wire::Seq(&[
                            Wire::Fixed(2),
                            Wire::Seq(&[
                                Wire::Fixed(16),
                                Wire::Fixed(4),
                                Wire::Fixed(1),
                                Wire::Fixed(1),
                            ]),
                        ]),
                    },
                },
                Child {
                    name: "replicatedRosterPlayer",
                    wire: Wire::Seq(&[
                        Wire::Seq(&[Wire::Text, Wire::Text, Wire::Fixed(1)]),
                        Wire::Fixed(16),
                        Wire::Fixed(2),
                        Wire::Fixed(1),
                        Wire::Fixed(1),
                    ]),
                },
                Child {
                    name: "replicatedRosterPlayer",
                    wire: Wire::Seq(&[
                        Wire::Seq(&[Wire::Text, Wire::Text, Wire::Fixed(1)]),
                        Wire::Fixed(16),
                        Wire::Fixed(2),
                        Wire::Fixed(1),
                        Wire::Fixed(1),
                    ]),
                },
                Child {
                    name: "replicatedRosterPlayer",
                    wire: Wire::Seq(&[
                        Wire::Seq(&[Wire::Text, Wire::Text, Wire::Fixed(1)]),
                        Wire::Fixed(16),
                        Wire::Fixed(2),
                        Wire::Fixed(1),
                        Wire::Fixed(1),
                    ]),
                },
                Child {
                    name: "replicatedRosterPlayer",
                    wire: Wire::Seq(&[
                        Wire::Seq(&[Wire::Text, Wire::Text, Wire::Fixed(1)]),
                        Wire::Fixed(16),
                        Wire::Fixed(2),
                        Wire::Fixed(1),
                        Wire::Fixed(1),
                    ]),
                },
                Child {
                    name: "replicatedRosterPlayer",
                    wire: Wire::Seq(&[
                        Wire::Seq(&[Wire::Text, Wire::Text, Wire::Fixed(1)]),
                        Wire::Fixed(16),
                        Wire::Fixed(2),
                        Wire::Fixed(1),
                        Wire::Fixed(1),
                    ]),
                },
                Child {
                    name: "replicatedRosterGroupData",
                    wire: Wire::Seq(&[
                        Wire::Fixed(1),
                        Wire::Fixed(1),
                        Wire::Fixed(1),
                        Wire::Fixed(1),
                    ]),
                },
                Child {
                    name: "replicatedRosterPlayer",
                    wire: Wire::Seq(&[
                        Wire::Seq(&[Wire::Text, Wire::Text, Wire::Fixed(1)]),
                        Wire::Fixed(16),
                        Wire::Fixed(2),
                        Wire::Fixed(1),
                        Wire::Fixed(1),
                    ]),
                },
                Child {
                    name: "replicatedRosterPlayer",
                    wire: Wire::Seq(&[
                        Wire::Seq(&[Wire::Text, Wire::Text, Wire::Fixed(1)]),
                        Wire::Fixed(16),
                        Wire::Fixed(2),
                        Wire::Fixed(1),
                        Wire::Fixed(1),
                    ]),
                },
                Child {
                    name: "replicatedRosterPlayer",
                    wire: Wire::Seq(&[
                        Wire::Seq(&[Wire::Text, Wire::Text, Wire::Fixed(1)]),
                        Wire::Fixed(16),
                        Wire::Fixed(2),
                        Wire::Fixed(1),
                        Wire::Fixed(1),
                    ]),
                },
                Child {
                    name: "replicatedRosterPlayer",
                    wire: Wire::Seq(&[
                        Wire::Seq(&[Wire::Text, Wire::Text, Wire::Fixed(1)]),
                        Wire::Fixed(16),
                        Wire::Fixed(2),
                        Wire::Fixed(1),
                        Wire::Fixed(1),
                    ]),
                },
                Child {
                    name: "replicatedRosterPlayer",
                    wire: Wire::Seq(&[
                        Wire::Seq(&[Wire::Text, Wire::Text, Wire::Fixed(1)]),
                        Wire::Fixed(16),
                        Wire::Fixed(2),
                        Wire::Fixed(1),
                        Wire::Fixed(1),
                    ]),
                },
                Child {
                    name: "replicatedRosterGroupData",
                    wire: Wire::Seq(&[
                        Wire::Fixed(1),
                        Wire::Fixed(1),
                        Wire::Fixed(1),
                        Wire::Fixed(1),
                    ]),
                },
                Child {
                    name: "replicatedRosterPlayer",
                    wire: Wire::Seq(&[
                        Wire::Seq(&[Wire::Text, Wire::Text, Wire::Fixed(1)]),
                        Wire::Fixed(16),
                        Wire::Fixed(2),
                        Wire::Fixed(1),
                        Wire::Fixed(1),
                    ]),
                },
                Child {
                    name: "replicatedRosterPlayer",
                    wire: Wire::Seq(&[
                        Wire::Seq(&[Wire::Text, Wire::Text, Wire::Fixed(1)]),
                        Wire::Fixed(16),
                        Wire::Fixed(2),
                        Wire::Fixed(1),
                        Wire::Fixed(1),
                    ]),
                },
                Child {
                    name: "replicatedRosterPlayer",
                    wire: Wire::Seq(&[
                        Wire::Seq(&[Wire::Text, Wire::Text, Wire::Fixed(1)]),
                        Wire::Fixed(16),
                        Wire::Fixed(2),
                        Wire::Fixed(1),
                        Wire::Fixed(1),
                    ]),
                },
                Child {
                    name: "replicatedRosterPlayer",
                    wire: Wire::Seq(&[
                        Wire::Seq(&[Wire::Text, Wire::Text, Wire::Fixed(1)]),
                        Wire::Fixed(16),
                        Wire::Fixed(2),
                        Wire::Fixed(1),
                        Wire::Fixed(1),
                    ]),
                },
                Child {
                    name: "replicatedRosterPlayer",
                    wire: Wire::Seq(&[
                        Wire::Seq(&[Wire::Text, Wire::Text, Wire::Fixed(1)]),
                        Wire::Fixed(16),
                        Wire::Fixed(2),
                        Wire::Fixed(1),
                        Wire::Fixed(1),
                    ]),
                },
                Child {
                    name: "replicatedRosterGroupData",
                    wire: Wire::Seq(&[
                        Wire::Fixed(1),
                        Wire::Fixed(1),
                        Wire::Fixed(1),
                        Wire::Fixed(1),
                    ]),
                },
                Child {
                    name: "replicatedRosterPlayer",
                    wire: Wire::Seq(&[
                        Wire::Seq(&[Wire::Text, Wire::Text, Wire::Fixed(1)]),
                        Wire::Fixed(16),
                        Wire::Fixed(2),
                        Wire::Fixed(1),
                        Wire::Fixed(1),
                    ]),
                },
                Child {
                    name: "replicatedRosterPlayer",
                    wire: Wire::Seq(&[
                        Wire::Seq(&[Wire::Text, Wire::Text, Wire::Fixed(1)]),
                        Wire::Fixed(16),
                        Wire::Fixed(2),
                        Wire::Fixed(1),
                        Wire::Fixed(1),
                    ]),
                },
                Child {
                    name: "replicatedRosterPlayer",
                    wire: Wire::Seq(&[
                        Wire::Seq(&[Wire::Text, Wire::Text, Wire::Fixed(1)]),
                        Wire::Fixed(16),
                        Wire::Fixed(2),
                        Wire::Fixed(1),
                        Wire::Fixed(1),
                    ]),
                },
                Child {
                    name: "replicatedRosterPlayer",
                    wire: Wire::Seq(&[
                        Wire::Seq(&[Wire::Text, Wire::Text, Wire::Fixed(1)]),
                        Wire::Fixed(16),
                        Wire::Fixed(2),
                        Wire::Fixed(1),
                        Wire::Fixed(1),
                    ]),
                },
                Child {
                    name: "replicatedRosterPlayer",
                    wire: Wire::Seq(&[
                        Wire::Seq(&[Wire::Text, Wire::Text, Wire::Fixed(1)]),
                        Wire::Fixed(16),
                        Wire::Fixed(2),
                        Wire::Fixed(1),
                        Wire::Fixed(1),
                    ]),
                },
                Child {
                    name: "replicatedRosterGroupData",
                    wire: Wire::Seq(&[
                        Wire::Fixed(1),
                        Wire::Fixed(1),
                        Wire::Fixed(1),
                        Wire::Fixed(1),
                    ]),
                },
                Child {
                    name: "replicatedRosterPlayer",
                    wire: Wire::Seq(&[
                        Wire::Seq(&[Wire::Text, Wire::Text, Wire::Fixed(1)]),
                        Wire::Fixed(16),
                        Wire::Fixed(2),
                        Wire::Fixed(1),
                        Wire::Fixed(1),
                    ]),
                },
                Child {
                    name: "replicatedRosterPlayer",
                    wire: Wire::Seq(&[
                        Wire::Seq(&[Wire::Text, Wire::Text, Wire::Fixed(1)]),
                        Wire::Fixed(16),
                        Wire::Fixed(2),
                        Wire::Fixed(1),
                        Wire::Fixed(1),
                    ]),
                },
                Child {
                    name: "replicatedRosterPlayer",
                    wire: Wire::Seq(&[
                        Wire::Seq(&[Wire::Text, Wire::Text, Wire::Fixed(1)]),
                        Wire::Fixed(16),
                        Wire::Fixed(2),
                        Wire::Fixed(1),
                        Wire::Fixed(1),
                    ]),
                },
                Child {
                    name: "replicatedRosterPlayer",
                    wire: Wire::Seq(&[
                        Wire::Seq(&[Wire::Text, Wire::Text, Wire::Fixed(1)]),
                        Wire::Fixed(16),
                        Wire::Fixed(2),
                        Wire::Fixed(1),
                        Wire::Fixed(1),
                    ]),
                },
                Child {
                    name: "replicatedRosterPlayer",
                    wire: Wire::Seq(&[
                        Wire::Seq(&[Wire::Text, Wire::Text, Wire::Fixed(1)]),
                        Wire::Fixed(16),
                        Wire::Fixed(2),
                        Wire::Fixed(1),
                        Wire::Fixed(1),
                    ]),
                },
                Child {
                    name: "replicatedRosterGroupData",
                    wire: Wire::Seq(&[
                        Wire::Fixed(1),
                        Wire::Fixed(1),
                        Wire::Fixed(1),
                        Wire::Fixed(1),
                    ]),
                },
                Child {
                    name: "replicatedRosterPlayer",
                    wire: Wire::Seq(&[
                        Wire::Seq(&[Wire::Text, Wire::Text, Wire::Fixed(1)]),
                        Wire::Fixed(16),
                        Wire::Fixed(2),
                        Wire::Fixed(1),
                        Wire::Fixed(1),
                    ]),
                },
                Child {
                    name: "replicatedRosterPlayer",
                    wire: Wire::Seq(&[
                        Wire::Seq(&[Wire::Text, Wire::Text, Wire::Fixed(1)]),
                        Wire::Fixed(16),
                        Wire::Fixed(2),
                        Wire::Fixed(1),
                        Wire::Fixed(1),
                    ]),
                },
                Child {
                    name: "replicatedRosterPlayer",
                    wire: Wire::Seq(&[
                        Wire::Seq(&[Wire::Text, Wire::Text, Wire::Fixed(1)]),
                        Wire::Fixed(16),
                        Wire::Fixed(2),
                        Wire::Fixed(1),
                        Wire::Fixed(1),
                    ]),
                },
                Child {
                    name: "replicatedRosterPlayer",
                    wire: Wire::Seq(&[
                        Wire::Seq(&[Wire::Text, Wire::Text, Wire::Fixed(1)]),
                        Wire::Fixed(16),
                        Wire::Fixed(2),
                        Wire::Fixed(1),
                        Wire::Fixed(1),
                    ]),
                },
                Child {
                    name: "replicatedRosterPlayer",
                    wire: Wire::Seq(&[
                        Wire::Seq(&[Wire::Text, Wire::Text, Wire::Fixed(1)]),
                        Wire::Fixed(16),
                        Wire::Fixed(2),
                        Wire::Fixed(1),
                        Wire::Fixed(1),
                    ]),
                },
                Child {
                    name: "replicatedRosterGroupData",
                    wire: Wire::Seq(&[
                        Wire::Fixed(1),
                        Wire::Fixed(1),
                        Wire::Fixed(1),
                        Wire::Fixed(1),
                    ]),
                },
                Child {
                    name: "replicatedRosterPlayer",
                    wire: Wire::Seq(&[
                        Wire::Seq(&[Wire::Text, Wire::Text, Wire::Fixed(1)]),
                        Wire::Fixed(16),
                        Wire::Fixed(2),
                        Wire::Fixed(1),
                        Wire::Fixed(1),
                    ]),
                },
                Child {
                    name: "replicatedRosterPlayer",
                    wire: Wire::Seq(&[
                        Wire::Seq(&[Wire::Text, Wire::Text, Wire::Fixed(1)]),
                        Wire::Fixed(16),
                        Wire::Fixed(2),
                        Wire::Fixed(1),
                        Wire::Fixed(1),
                    ]),
                },
                Child {
                    name: "replicatedRosterPlayer",
                    wire: Wire::Seq(&[
                        Wire::Seq(&[Wire::Text, Wire::Text, Wire::Fixed(1)]),
                        Wire::Fixed(16),
                        Wire::Fixed(2),
                        Wire::Fixed(1),
                        Wire::Fixed(1),
                    ]),
                },
                Child {
                    name: "replicatedRosterPlayer",
                    wire: Wire::Seq(&[
                        Wire::Seq(&[Wire::Text, Wire::Text, Wire::Fixed(1)]),
                        Wire::Fixed(16),
                        Wire::Fixed(2),
                        Wire::Fixed(1),
                        Wire::Fixed(1),
                    ]),
                },
                Child {
                    name: "replicatedRosterPlayer",
                    wire: Wire::Seq(&[
                        Wire::Seq(&[Wire::Text, Wire::Text, Wire::Fixed(1)]),
                        Wire::Fixed(16),
                        Wire::Fixed(2),
                        Wire::Fixed(1),
                        Wire::Fixed(1),
                    ]),
                },
                Child {
                    name: "replicatedRosterGroupData",
                    wire: Wire::Seq(&[
                        Wire::Fixed(1),
                        Wire::Fixed(1),
                        Wire::Fixed(1),
                        Wire::Fixed(1),
                    ]),
                },
                Child {
                    name: "replicatedRosterPlayer",
                    wire: Wire::Seq(&[
                        Wire::Seq(&[Wire::Text, Wire::Text, Wire::Fixed(1)]),
                        Wire::Fixed(16),
                        Wire::Fixed(2),
                        Wire::Fixed(1),
                        Wire::Fixed(1),
                    ]),
                },
                Child {
                    name: "replicatedRosterPlayer",
                    wire: Wire::Seq(&[
                        Wire::Seq(&[Wire::Text, Wire::Text, Wire::Fixed(1)]),
                        Wire::Fixed(16),
                        Wire::Fixed(2),
                        Wire::Fixed(1),
                        Wire::Fixed(1),
                    ]),
                },
                Child {
                    name: "replicatedRosterPlayer",
                    wire: Wire::Seq(&[
                        Wire::Seq(&[Wire::Text, Wire::Text, Wire::Fixed(1)]),
                        Wire::Fixed(16),
                        Wire::Fixed(2),
                        Wire::Fixed(1),
                        Wire::Fixed(1),
                    ]),
                },
                Child {
                    name: "replicatedRosterPlayer",
                    wire: Wire::Seq(&[
                        Wire::Seq(&[Wire::Text, Wire::Text, Wire::Fixed(1)]),
                        Wire::Fixed(16),
                        Wire::Fixed(2),
                        Wire::Fixed(1),
                        Wire::Fixed(1),
                    ]),
                },
                Child {
                    name: "replicatedRosterPlayer",
                    wire: Wire::Seq(&[
                        Wire::Seq(&[Wire::Text, Wire::Text, Wire::Fixed(1)]),
                        Wire::Fixed(16),
                        Wire::Fixed(2),
                        Wire::Fixed(1),
                        Wire::Fixed(1),
                    ]),
                },
                Child {
                    name: "replicatedRosterGroupData",
                    wire: Wire::Seq(&[
                        Wire::Fixed(1),
                        Wire::Fixed(1),
                        Wire::Fixed(1),
                        Wire::Fixed(1),
                    ]),
                },
                Child {
                    name: "replicatedRosterPlayer",
                    wire: Wire::Seq(&[
                        Wire::Seq(&[Wire::Text, Wire::Text, Wire::Fixed(1)]),
                        Wire::Fixed(16),
                        Wire::Fixed(2),
                        Wire::Fixed(1),
                        Wire::Fixed(1),
                    ]),
                },
                Child {
                    name: "replicatedRosterPlayer",
                    wire: Wire::Seq(&[
                        Wire::Seq(&[Wire::Text, Wire::Text, Wire::Fixed(1)]),
                        Wire::Fixed(16),
                        Wire::Fixed(2),
                        Wire::Fixed(1),
                        Wire::Fixed(1),
                    ]),
                },
                Child {
                    name: "replicatedRosterPlayer",
                    wire: Wire::Seq(&[
                        Wire::Seq(&[Wire::Text, Wire::Text, Wire::Fixed(1)]),
                        Wire::Fixed(16),
                        Wire::Fixed(2),
                        Wire::Fixed(1),
                        Wire::Fixed(1),
                    ]),
                },
                Child {
                    name: "replicatedRosterPlayer",
                    wire: Wire::Seq(&[
                        Wire::Seq(&[Wire::Text, Wire::Text, Wire::Fixed(1)]),
                        Wire::Fixed(16),
                        Wire::Fixed(2),
                        Wire::Fixed(1),
                        Wire::Fixed(1),
                    ]),
                },
                Child {
                    name: "replicatedRosterPlayer",
                    wire: Wire::Seq(&[
                        Wire::Seq(&[Wire::Text, Wire::Text, Wire::Fixed(1)]),
                        Wire::Fixed(16),
                        Wire::Fixed(2),
                        Wire::Fixed(1),
                        Wire::Fixed(1),
                    ]),
                },
                Child {
                    name: "replicatedRosterGroupData",
                    wire: Wire::Seq(&[
                        Wire::Fixed(1),
                        Wire::Fixed(1),
                        Wire::Fixed(1),
                        Wire::Fixed(1),
                    ]),
                },
                Child {
                    name: "replicatedRosterPlayer",
                    wire: Wire::Seq(&[
                        Wire::Seq(&[Wire::Text, Wire::Text, Wire::Fixed(1)]),
                        Wire::Fixed(16),
                        Wire::Fixed(2),
                        Wire::Fixed(1),
                        Wire::Fixed(1),
                    ]),
                },
                Child {
                    name: "replicatedRosterPlayer",
                    wire: Wire::Seq(&[
                        Wire::Seq(&[Wire::Text, Wire::Text, Wire::Fixed(1)]),
                        Wire::Fixed(16),
                        Wire::Fixed(2),
                        Wire::Fixed(1),
                        Wire::Fixed(1),
                    ]),
                },
                Child {
                    name: "replicatedRosterPlayer",
                    wire: Wire::Seq(&[
                        Wire::Seq(&[Wire::Text, Wire::Text, Wire::Fixed(1)]),
                        Wire::Fixed(16),
                        Wire::Fixed(2),
                        Wire::Fixed(1),
                        Wire::Fixed(1),
                    ]),
                },
                Child {
                    name: "replicatedRosterPlayer",
                    wire: Wire::Seq(&[
                        Wire::Seq(&[Wire::Text, Wire::Text, Wire::Fixed(1)]),
                        Wire::Fixed(16),
                        Wire::Fixed(2),
                        Wire::Fixed(1),
                        Wire::Fixed(1),
                    ]),
                },
                Child {
                    name: "replicatedRosterPlayer",
                    wire: Wire::Seq(&[
                        Wire::Seq(&[Wire::Text, Wire::Text, Wire::Fixed(1)]),
                        Wire::Fixed(16),
                        Wire::Fixed(2),
                        Wire::Fixed(1),
                        Wire::Fixed(1),
                    ]),
                },
                Child {
                    name: "replicatedRosterGroupData",
                    wire: Wire::Seq(&[
                        Wire::Fixed(1),
                        Wire::Fixed(1),
                        Wire::Fixed(1),
                        Wire::Fixed(1),
                    ]),
                },
                Child {
                    name: "replicatedBackfillEnabledState",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "kickedRaidMemberCooldown",
                    wire: Wire::Fixed(8),
                },
            ],
            &[],
        ],
    },
    ClassLayout {
        class: 1927,
        name: "MB::ReactionTrackingReplicatedState",
        slots: &[
            &[
                Child {
                    name: "reactionRefCount",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "damagingGDEID",
                    wire: Wire::Seq(&[Wire::Fixed(1), Wire::Fixed(8)]),
                },
                Child {
                    name: "damageTableId",
                    wire: Wire::Fixed(4),
                },
                Child {
                    name: "damageTableRow",
                    wire: Wire::Fixed(2),
                },
                Child {
                    name: "damagedDir",
                    wire: Wire::Seq(&[Wire::Fixed(2), Wire::Fixed(2), Wire::Fixed(2)]),
                },
                Child {
                    name: "impactDist",
                    wire: Wire::Seq(&[Wire::Fixed(2), Wire::Fixed(2), Wire::Fixed(2)]),
                },
                Child {
                    name: "stunToBreakout",
                    wire: Wire::Fixed(4),
                },
                Child {
                    name: "reactionFlags",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "attackerLevel",
                    wire: Wire::Fixed(1),
                },
            ],
            &[],
        ],
    },
    ClassLayout {
        class: 1982,
        name: "MB::ContributionComponentReplicatedState",
        slots: &[
            &[Child {
                name: "xpEvents",
                wire: Wire::List {
                    value: &Wire::Seq(&[
                        Wire::Fixed(8),
                        Wire::Seq(&[
                            Wire::Fixed(4),
                            Wire::Fixed(4),
                            Wire::Fixed(4),
                            Wire::Fixed(4),
                            Wire::Fixed(1),
                            Wire::Fixed(4),
                            Wire::Fixed(4),
                            Wire::Seq(&[
                                Wire::Fixed(4),
                                Wire::Array(&Wire::Seq(&[
                                    Wire::Fixed(4),
                                    Wire::Fixed(2),
                                    Wire::Seq(&[
                                        Wire::Fixed(4),
                                        Wire::Fixed(1),
                                        Wire::Fixed(4),
                                        Wire::Fixed(4),
                                        Wire::Fixed(1),
                                        Wire::Fixed(1),
                                        Wire::Seq(&[
                                            Wire::Array(&Wire::Fixed(4)),
                                            Wire::Array(&Wire::Fixed(4)),
                                            Wire::Fixed(1),
                                            Wire::Array(&Wire::Fixed(4)),
                                        ]),
                                    ]),
                                ])),
                                Wire::Fixed(1),
                                Wire::Fixed(1),
                                Wire::Fixed(4),
                                Wire::Fixed(2),
                                Wire::Fixed(4),
                                Wire::Fixed(4),
                            ]),
                        ]),
                    ]),
                    key: &Wire::Wide,
                    full: &Wire::Seq(&[
                        Wire::Fixed(8),
                        Wire::Seq(&[
                            Wire::Fixed(4),
                            Wire::Fixed(4),
                            Wire::Fixed(4),
                            Wire::Fixed(4),
                            Wire::Fixed(1),
                            Wire::Fixed(4),
                            Wire::Fixed(4),
                            Wire::Seq(&[
                                Wire::Fixed(4),
                                Wire::Array(&Wire::Seq(&[
                                    Wire::Fixed(4),
                                    Wire::Fixed(2),
                                    Wire::Seq(&[
                                        Wire::Fixed(4),
                                        Wire::Fixed(1),
                                        Wire::Fixed(4),
                                        Wire::Fixed(4),
                                        Wire::Fixed(1),
                                        Wire::Fixed(1),
                                        Wire::Seq(&[
                                            Wire::Array(&Wire::Fixed(4)),
                                            Wire::Array(&Wire::Fixed(4)),
                                            Wire::Fixed(1),
                                            Wire::Array(&Wire::Fixed(4)),
                                        ]),
                                    ]),
                                ])),
                                Wire::Fixed(1),
                                Wire::Fixed(1),
                                Wire::Fixed(4),
                                Wire::Fixed(2),
                                Wire::Fixed(4),
                                Wire::Fixed(4),
                            ]),
                        ]),
                    ]),
                },
            }],
            &[],
        ],
    },
    ClassLayout {
        class: 1994,
        name: "Javelin::GroupsComponentReplicatedState",
        slots: &[
            &[
                Child {
                    name: "groupId",
                    wire: Wire::Fixed(16),
                },
                Child {
                    name: "raidId",
                    wire: Wire::Fixed(8),
                },
                Child {
                    name: "opposingRaidId",
                    wire: Wire::Fixed(8),
                },
            ],
            &[
                Child {
                    name: "raidType",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "groupFinderGroupId",
                    wire: Wire::Fixed(16),
                },
                Child {
                    name: "isGroupFinderGroupCreator",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "createSource",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "groupFinderApplications",
                    wire: Wire::List {
                        value: &Wire::Seq(&[Wire::Fixed(1), Wire::Fixed(1)]),
                        key: &Wire::Fixed(16),
                        full: &Wire::Seq(&[Wire::Fixed(16), Wire::Fixed(1), Wire::Fixed(1)]),
                    },
                },
                Child {
                    name: "inboundGroupInvites",
                    wire: Wire::List {
                        value: &Wire::Seq(&[
                            Wire::Fixed(16),
                            Wire::Fixed(16),
                            Wire::Seq(&[
                                Wire::Fixed(8),
                                Wire::Fixed(1),
                                Wire::FlagId,
                                Wire::FlagId,
                                Wire::Fixed(8),
                                Wire::Fixed(1),
                                Wire::Fixed(8),
                                Wire::Seq(&[
                                    Wire::Fixed(16),
                                    Wire::Fixed(1),
                                    Wire::Fixed(1),
                                    Wire::Fixed(1),
                                    Wire::Fixed(4),
                                    Wire::Fixed(1),
                                    Wire::Seq(&[Wire::Fixed(1), Wire::Fixed(1)]),
                                ]),
                            ]),
                        ]),
                        key: &Wire::Fixed(16),
                        full: &Wire::Seq(&[
                            Wire::Fixed(16),
                            Wire::Seq(&[
                                Wire::Fixed(16),
                                Wire::Fixed(16),
                                Wire::Seq(&[
                                    Wire::Fixed(8),
                                    Wire::Fixed(1),
                                    Wire::FlagId,
                                    Wire::FlagId,
                                    Wire::Fixed(8),
                                    Wire::Fixed(1),
                                    Wire::Fixed(8),
                                    Wire::Seq(&[
                                        Wire::Fixed(16),
                                        Wire::Fixed(1),
                                        Wire::Fixed(1),
                                        Wire::Fixed(1),
                                        Wire::Fixed(4),
                                        Wire::Fixed(1),
                                        Wire::Seq(&[Wire::Fixed(1), Wire::Fixed(1)]),
                                    ]),
                                ]),
                            ]),
                        ]),
                    },
                },
                Child {
                    name: "outboundGroupInvites",
                    wire: Wire::List {
                        value: &Wire::Seq(&[
                            Wire::Fixed(16),
                            Wire::Fixed(16),
                            Wire::Seq(&[
                                Wire::Fixed(8),
                                Wire::Fixed(1),
                                Wire::FlagId,
                                Wire::FlagId,
                                Wire::Fixed(8),
                                Wire::Fixed(1),
                                Wire::Fixed(8),
                                Wire::Seq(&[
                                    Wire::Fixed(16),
                                    Wire::Fixed(1),
                                    Wire::Fixed(1),
                                    Wire::Fixed(1),
                                    Wire::Fixed(4),
                                    Wire::Fixed(1),
                                    Wire::Seq(&[Wire::Fixed(1), Wire::Fixed(1)]),
                                ]),
                            ]),
                        ]),
                        key: &Wire::Fixed(16),
                        full: &Wire::Seq(&[
                            Wire::Fixed(16),
                            Wire::Seq(&[
                                Wire::Fixed(16),
                                Wire::Fixed(16),
                                Wire::Seq(&[
                                    Wire::Fixed(8),
                                    Wire::Fixed(1),
                                    Wire::FlagId,
                                    Wire::FlagId,
                                    Wire::Fixed(8),
                                    Wire::Fixed(1),
                                    Wire::Fixed(8),
                                    Wire::Seq(&[
                                        Wire::Fixed(16),
                                        Wire::Fixed(1),
                                        Wire::Fixed(1),
                                        Wire::Fixed(1),
                                        Wire::Fixed(4),
                                        Wire::Fixed(1),
                                        Wire::Seq(&[Wire::Fixed(1), Wire::Fixed(1)]),
                                    ]),
                                ]),
                            ]),
                        ]),
                    },
                },
                Child {
                    name: "nextEligibleAbandonGameModeVoteTime",
                    wire: Wire::Fixed(8),
                },
                Child {
                    name: "gameInviteData",
                    wire: Wire::Seq(&[Wire::Fixed(16), Wire::FlagId, Wire::Fixed(8)]),
                },
                Child {
                    name: "isGroupPristine",
                    wire: Wire::Fixed(1),
                },
            ],
            &[],
        ],
    },
    ClassLayout {
        class: 2027,
        name: "MB::LootDropReplicatedState",
        slots: &[
            &[Child {
                name: "m_rarityLevel",
                wire: Wire::Fixed(4),
            }],
            &[],
        ],
    },
    ClassLayout {
        class: 2133,
        name: "MB::EncounterComponentReplicatedState",
        slots: &[
            &[Child {
                name: "status",
                wire: Wire::Array(&Wire::Seq(&[Wire::Fixed(4), Wire::Fixed(4)])),
            }],
            &[],
        ],
    },
    ClassLayout {
        class: 2187,
        name: "MB::PlacementObstructionComponentReplicatedState",
        slots: &[
            &[Child {
                name: "hasCompletionObstruction",
                wire: Wire::Fixed(1),
            }],
            &[],
        ],
    },
    ClassLayout {
        class: 2201,
        name: "Javelin::GameModeMutationSchedulerReplicatedState",
        slots: &[
            &[
                Child {
                    name: "gameModeMutationSets",
                    wire: Wire::List {
                        value: &Wire::Seq(&[
                            Wire::Fixed(4),
                            Wire::Seq(&[Wire::Fixed(4), Wire::Fixed(4)]),
                        ]),
                        key: &Wire::Fixed(4),
                        full: &Wire::Seq(&[
                            Wire::Fixed(4),
                            Wire::Seq(&[
                                Wire::Fixed(4),
                                Wire::Seq(&[Wire::Fixed(4), Wire::Fixed(4)]),
                            ]),
                        ]),
                    },
                },
                Child {
                    name: "mutationCadenceStartTime",
                    wire: Wire::Fixed(8),
                },
            ],
            &[],
        ],
    },
    ClassLayout {
        class: 2267,
        name: "MB::AbilityInstanceTrackingComponentReplicatedState",
        slots: &[
            &[
                Child {
                    name: "m_abilityInstances",
                    wire: Wire::Seq(&[Wire::Fixed(8), Wire::Fixed(16), Wire::Fixed(8)]),
                },
                Child {
                    name: "m_abilityInstances",
                    wire: Wire::Seq(&[Wire::Fixed(8), Wire::Fixed(16), Wire::Fixed(8)]),
                },
                Child {
                    name: "m_abilityInstances",
                    wire: Wire::Seq(&[Wire::Fixed(8), Wire::Fixed(16), Wire::Fixed(8)]),
                },
                Child {
                    name: "m_abilityInstances",
                    wire: Wire::Seq(&[Wire::Fixed(8), Wire::Fixed(16), Wire::Fixed(8)]),
                },
                Child {
                    name: "m_abilityInstances",
                    wire: Wire::Seq(&[Wire::Fixed(8), Wire::Fixed(16), Wire::Fixed(8)]),
                },
                Child {
                    name: "m_abilityInstances",
                    wire: Wire::Seq(&[Wire::Fixed(8), Wire::Fixed(16), Wire::Fixed(8)]),
                },
                Child {
                    name: "m_abilityInstances",
                    wire: Wire::Seq(&[Wire::Fixed(8), Wire::Fixed(16), Wire::Fixed(8)]),
                },
                Child {
                    name: "m_abilityInstances",
                    wire: Wire::Seq(&[Wire::Fixed(8), Wire::Fixed(16), Wire::Fixed(8)]),
                },
                Child {
                    name: "m_abilityInstances",
                    wire: Wire::Seq(&[Wire::Fixed(8), Wire::Fixed(16), Wire::Fixed(8)]),
                },
                Child {
                    name: "m_abilityInstances",
                    wire: Wire::Seq(&[Wire::Fixed(8), Wire::Fixed(16), Wire::Fixed(8)]),
                },
                Child {
                    name: "m_abilityInstances",
                    wire: Wire::Seq(&[Wire::Fixed(8), Wire::Fixed(16), Wire::Fixed(8)]),
                },
                Child {
                    name: "m_abilityInstances",
                    wire: Wire::Seq(&[Wire::Fixed(8), Wire::Fixed(16), Wire::Fixed(8)]),
                },
                Child {
                    name: "m_abilityInstances",
                    wire: Wire::Seq(&[Wire::Fixed(8), Wire::Fixed(16), Wire::Fixed(8)]),
                },
                Child {
                    name: "m_abilityInstances",
                    wire: Wire::Seq(&[Wire::Fixed(8), Wire::Fixed(16), Wire::Fixed(8)]),
                },
                Child {
                    name: "m_abilityInstances",
                    wire: Wire::Seq(&[Wire::Fixed(8), Wire::Fixed(16), Wire::Fixed(8)]),
                },
                Child {
                    name: "m_abilityInstances",
                    wire: Wire::Seq(&[Wire::Fixed(8), Wire::Fixed(16), Wire::Fixed(8)]),
                },
                Child {
                    name: "m_abilityInstances",
                    wire: Wire::Seq(&[Wire::Fixed(8), Wire::Fixed(16), Wire::Fixed(8)]),
                },
                Child {
                    name: "m_abilityInstances",
                    wire: Wire::Seq(&[Wire::Fixed(8), Wire::Fixed(16), Wire::Fixed(8)]),
                },
                Child {
                    name: "m_abilityInstances",
                    wire: Wire::Seq(&[Wire::Fixed(8), Wire::Fixed(16), Wire::Fixed(8)]),
                },
                Child {
                    name: "m_abilityInstances",
                    wire: Wire::Seq(&[Wire::Fixed(8), Wire::Fixed(16), Wire::Fixed(8)]),
                },
                Child {
                    name: "m_abilityInstances",
                    wire: Wire::Seq(&[Wire::Fixed(8), Wire::Fixed(16), Wire::Fixed(8)]),
                },
                Child {
                    name: "m_abilityInstances",
                    wire: Wire::Seq(&[Wire::Fixed(8), Wire::Fixed(16), Wire::Fixed(8)]),
                },
                Child {
                    name: "m_abilityInstances",
                    wire: Wire::Seq(&[Wire::Fixed(8), Wire::Fixed(16), Wire::Fixed(8)]),
                },
                Child {
                    name: "m_abilityInstances",
                    wire: Wire::Seq(&[Wire::Fixed(8), Wire::Fixed(16), Wire::Fixed(8)]),
                },
                Child {
                    name: "m_abilityInstances",
                    wire: Wire::Seq(&[Wire::Fixed(8), Wire::Fixed(16), Wire::Fixed(8)]),
                },
                Child {
                    name: "m_abilityInstances",
                    wire: Wire::Seq(&[Wire::Fixed(8), Wire::Fixed(16), Wire::Fixed(8)]),
                },
                Child {
                    name: "m_abilityInstances",
                    wire: Wire::Seq(&[Wire::Fixed(8), Wire::Fixed(16), Wire::Fixed(8)]),
                },
                Child {
                    name: "m_abilityInstances",
                    wire: Wire::Seq(&[Wire::Fixed(8), Wire::Fixed(16), Wire::Fixed(8)]),
                },
                Child {
                    name: "m_abilityInstances",
                    wire: Wire::Seq(&[Wire::Fixed(8), Wire::Fixed(16), Wire::Fixed(8)]),
                },
                Child {
                    name: "m_abilityInstances",
                    wire: Wire::Seq(&[Wire::Fixed(8), Wire::Fixed(16), Wire::Fixed(8)]),
                },
                Child {
                    name: "m_abilityInstances",
                    wire: Wire::Seq(&[Wire::Fixed(8), Wire::Fixed(16), Wire::Fixed(8)]),
                },
                Child {
                    name: "m_abilityInstances",
                    wire: Wire::Seq(&[Wire::Fixed(8), Wire::Fixed(16), Wire::Fixed(8)]),
                },
                Child {
                    name: "m_abilityInstances",
                    wire: Wire::Seq(&[Wire::Fixed(8), Wire::Fixed(16), Wire::Fixed(8)]),
                },
                Child {
                    name: "m_abilityInstances",
                    wire: Wire::Seq(&[Wire::Fixed(8), Wire::Fixed(16), Wire::Fixed(8)]),
                },
                Child {
                    name: "m_abilityInstances",
                    wire: Wire::Seq(&[Wire::Fixed(8), Wire::Fixed(16), Wire::Fixed(8)]),
                },
                Child {
                    name: "m_abilityInstances",
                    wire: Wire::Seq(&[Wire::Fixed(8), Wire::Fixed(16), Wire::Fixed(8)]),
                },
                Child {
                    name: "m_abilityInstances",
                    wire: Wire::Seq(&[Wire::Fixed(8), Wire::Fixed(16), Wire::Fixed(8)]),
                },
                Child {
                    name: "m_abilityInstances",
                    wire: Wire::Seq(&[Wire::Fixed(8), Wire::Fixed(16), Wire::Fixed(8)]),
                },
                Child {
                    name: "m_abilityInstances",
                    wire: Wire::Seq(&[Wire::Fixed(8), Wire::Fixed(16), Wire::Fixed(8)]),
                },
                Child {
                    name: "m_abilityInstances",
                    wire: Wire::Seq(&[Wire::Fixed(8), Wire::Fixed(16), Wire::Fixed(8)]),
                },
                Child {
                    name: "m_abilityInstances",
                    wire: Wire::Seq(&[Wire::Fixed(8), Wire::Fixed(16), Wire::Fixed(8)]),
                },
                Child {
                    name: "m_abilityInstances",
                    wire: Wire::Seq(&[Wire::Fixed(8), Wire::Fixed(16), Wire::Fixed(8)]),
                },
                Child {
                    name: "m_abilityInstances",
                    wire: Wire::Seq(&[Wire::Fixed(8), Wire::Fixed(16), Wire::Fixed(8)]),
                },
                Child {
                    name: "m_abilityInstances",
                    wire: Wire::Seq(&[Wire::Fixed(8), Wire::Fixed(16), Wire::Fixed(8)]),
                },
                Child {
                    name: "m_abilityInstances",
                    wire: Wire::Seq(&[Wire::Fixed(8), Wire::Fixed(16), Wire::Fixed(8)]),
                },
                Child {
                    name: "m_abilityInstances",
                    wire: Wire::Seq(&[Wire::Fixed(8), Wire::Fixed(16), Wire::Fixed(8)]),
                },
                Child {
                    name: "m_abilityInstances",
                    wire: Wire::Seq(&[Wire::Fixed(8), Wire::Fixed(16), Wire::Fixed(8)]),
                },
                Child {
                    name: "m_abilityInstances",
                    wire: Wire::Seq(&[Wire::Fixed(8), Wire::Fixed(16), Wire::Fixed(8)]),
                },
                Child {
                    name: "m_abilityInstances",
                    wire: Wire::Seq(&[Wire::Fixed(8), Wire::Fixed(16), Wire::Fixed(8)]),
                },
                Child {
                    name: "m_abilityInstances",
                    wire: Wire::Seq(&[Wire::Fixed(8), Wire::Fixed(16), Wire::Fixed(8)]),
                },
                Child {
                    name: "m_abilityInstances",
                    wire: Wire::Seq(&[Wire::Fixed(8), Wire::Fixed(16), Wire::Fixed(8)]),
                },
                Child {
                    name: "m_abilityInstances",
                    wire: Wire::Seq(&[Wire::Fixed(8), Wire::Fixed(16), Wire::Fixed(8)]),
                },
                Child {
                    name: "m_abilityInstances",
                    wire: Wire::Seq(&[Wire::Fixed(8), Wire::Fixed(16), Wire::Fixed(8)]),
                },
                Child {
                    name: "m_abilityInstances",
                    wire: Wire::Seq(&[Wire::Fixed(8), Wire::Fixed(16), Wire::Fixed(8)]),
                },
                Child {
                    name: "m_abilityInstances",
                    wire: Wire::Seq(&[Wire::Fixed(8), Wire::Fixed(16), Wire::Fixed(8)]),
                },
            ],
            &[],
        ],
    },
    ClassLayout {
        class: 2270,
        name: "MB::BuilderComponentReplicatedState",
        slots: &[
            &[Child {
                name: "playerBuilderState",
                wire: Wire::Fixed(1),
            }],
            &[Child {
                name: "numberOfPlacedDeployables",
                wire: Wire::Fixed(1),
            }],
            &[],
        ],
    },
    ClassLayout {
        class: 2330,
        name: "MB::DoorComponentReplicatedState",
        slots: &[
            &[Child {
                name: "doorState",
                wire: Wire::Fixed(1),
            }],
            &[],
        ],
    },
    ClassLayout {
        class: 2343,
        name: "Javelin::GameModeReplicatedState",
        slots: &[
            &[
                Child {
                    name: "curScriptStateId",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "curScriptId",
                    wire: Wire::Fixed(4),
                },
                Child {
                    name: "spawnedEntityIdsBySpawnerId",
                    wire: Wire::List {
                        value: &Wire::Fixed(8),
                        key: &Wire::Fixed(4),
                        full: &Wire::Seq(&[Wire::Fixed(4), Wire::Fixed(8)]),
                    },
                },
                Child {
                    name: "gameModeId",
                    wire: Wire::Fixed(4),
                },
                Child {
                    name: "gameModeMapId",
                    wire: Wire::Fixed(4),
                },
                Child {
                    name: "participantFacetRefs",
                    wire: Wire::List {
                        value: &Wire::Seq(&[Wire::Fixed(16), Wire::Fixed(8), Wire::Fixed(8)]),
                        key: &Wire::Wide,
                        full: &Wire::Seq(&[Wire::Fixed(16), Wire::Fixed(8), Wire::Fixed(8)]),
                    },
                },
                Child {
                    name: "participantStatuses",
                    wire: Wire::List {
                        value: &Wire::Fixed(1),
                        key: &Wire::Wide,
                        full: &Wire::Fixed(1),
                    },
                },
                Child {
                    name: "participantTeamIndexes",
                    wire: Wire::List {
                        value: &Wire::Fixed(1),
                        key: &Wire::Wide,
                        full: &Wire::Fixed(1),
                    },
                },
                Child {
                    name: "participantCharacterIds",
                    wire: Wire::List {
                        value: &Wire::FlagId,
                        key: &Wire::Wide,
                        full: &Wire::FlagId,
                    },
                },
                Child {
                    name: "raidIds",
                    wire: Wire::List {
                        value: &Wire::Fixed(8),
                        key: &Wire::Wide,
                        full: &Wire::Fixed(8),
                    },
                },
                Child {
                    name: "values",
                    wire: Wire::List {
                        value: &Wire::Wide,
                        key: &Wire::Fixed(4),
                        full: &Wire::Seq(&[Wire::Fixed(4), Wire::Fixed(8)]),
                    },
                },
                Child {
                    name: "syncedTimers",
                    wire: Wire::List {
                        value: &Wire::Wide,
                        key: &Wire::Fixed(4),
                        full: &Wire::Seq(&[Wire::Fixed(4), Wire::Fixed(8)]),
                    },
                },
                Child {
                    name: "Event1",
                    wire: Wire::Seq(&[Wire::Fixed(4), Wire::Fixed(8), Wire::Fixed(4)]),
                },
                Child {
                    name: "Event2",
                    wire: Wire::Seq(&[Wire::Fixed(4), Wire::Fixed(8), Wire::Fixed(4)]),
                },
                Child {
                    name: "Event3",
                    wire: Wire::Seq(&[Wire::Fixed(4), Wire::Fixed(8), Wire::Fixed(4)]),
                },
                Child {
                    name: "Event4",
                    wire: Wire::Seq(&[Wire::Fixed(4), Wire::Fixed(8), Wire::Fixed(4)]),
                },
                Child {
                    name: "Event5",
                    wire: Wire::Seq(&[Wire::Fixed(4), Wire::Fixed(8), Wire::Fixed(4)]),
                },
                Child {
                    name: "Event6",
                    wire: Wire::Seq(&[Wire::Fixed(4), Wire::Fixed(8), Wire::Fixed(4)]),
                },
                Child {
                    name: "Event7",
                    wire: Wire::Seq(&[Wire::Fixed(4), Wire::Fixed(8), Wire::Fixed(4)]),
                },
                Child {
                    name: "Event8",
                    wire: Wire::Seq(&[Wire::Fixed(4), Wire::Fixed(8), Wire::Fixed(4)]),
                },
                Child {
                    name: "Event9",
                    wire: Wire::Seq(&[Wire::Fixed(4), Wire::Fixed(8), Wire::Fixed(4)]),
                },
                Child {
                    name: "Event10",
                    wire: Wire::Seq(&[Wire::Fixed(4), Wire::Fixed(8), Wire::Fixed(4)]),
                },
                Child {
                    name: "globalAfflictionData",
                    wire: Wire::Seq(&[
                        Wire::Fixed(4),
                        Wire::Fixed(8),
                        Wire::Fixed(4),
                        Wire::Fixed(8),
                        Wire::Fixed(4),
                        Wire::Seq(&[Wire::Fixed(1), Wire::Fixed(1)]),
                    ]),
                },
                Child {
                    name: "mapOrigin",
                    wire: Wire::Fixed(4),
                },
                Child {
                    name: "tileSizeMeters",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "mapSizeInTiles",
                    wire: Wire::Fixed(2),
                },
                Child {
                    name: "tileUiFilenameIdAndRotation",
                    wire: Wire::List {
                        value: &Wire::Fixed(1),
                        key: &Wire::Wide,
                        full: &Wire::Fixed(1),
                    },
                },
                Child {
                    name: "tileVisited",
                    wire: Wire::List {
                        value: &Wire::Fixed(1),
                        key: &Wire::Wide,
                        full: &Wire::Fixed(1),
                    },
                },
                Child {
                    name: "icons",
                    wire: Wire::List {
                        value: &Wire::Seq(&[Wire::Fixed(4), Wire::Fixed(8)]),
                        key: &Wire::Wide,
                        full: &Wire::Seq(&[Wire::Fixed(4), Wire::Fixed(8)]),
                    },
                },
                Child {
                    name: "linkedMode",
                    wire: Wire::Fixed(1),
                },
            ],
            &[],
            &[],
        ],
    },
    ClassLayout {
        class: 2406,
        name: "Javelin::PlayerArenaReplicatedState",
        slots: &[
            &[
                Child {
                    name: "isInArena",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "isQueuedForDungeon",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "dungeonCooldownTime",
                    wire: Wire::Fixed(8),
                },
                Child {
                    name: "enterSoloTrialCooldownTime",
                    wire: Wire::Fixed(8),
                },
                Child {
                    name: "dungeonRanks",
                    wire: Wire::List {
                        value: &Wire::Fixed(1),
                        key: &Wire::Wide,
                        full: &Wire::Fixed(1),
                    },
                },
                Child {
                    name: "m_lastDungeonsEntered",
                    wire: Wire::Fixed(16),
                },
                Child {
                    name: "m_lastMutatedDungeonEntered",
                    wire: Wire::Fixed(16),
                },
                Child {
                    name: "m_numBaseDungeonsEnteredSinceLastRefresh",
                    wire: Wire::Fixed(4),
                },
                Child {
                    name: "m_numMutatedDungeonsEnteredSinceLastRefresh",
                    wire: Wire::Fixed(4),
                },
                Child {
                    name: "m_numGroupTrialsEnteredSinceLastRefresh",
                    wire: Wire::Fixed(4),
                },
                Child {
                    name: "m_nextDungeonBaseMaxLimitRefreshTime",
                    wire: Wire::Fixed(8),
                },
                Child {
                    name: "m_nextDungeonMutatedMaxLimitRefreshTime",
                    wire: Wire::Fixed(8),
                },
                Child {
                    name: "m_nextGroupTrialMaxLimitRefreshTime",
                    wire: Wire::Fixed(8),
                },
                Child {
                    name: "hasMutationUnlockAwardBeenGranted",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "m_singlePlayerInstanceState",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "m_singlePlayerDungeonTime",
                    wire: Wire::Fixed(8),
                },
                Child {
                    name: "m_gameModeIdx",
                    wire: Wire::Fixed(1),
                },
            ],
            &[],
        ],
    },
    ClassLayout {
        class: 2443,
        name: "MB::LookTargetingComponentReplicatedState",
        slots: &[
            &[Child {
                name: "currentLookAtTarget",
                wire: Wire::Fixed(8),
            }],
            &[],
        ],
    },
    ClassLayout {
        class: 2768,
        name: "MB::PlayerHousingReplicatedState",
        slots: &[
            &[
                Child {
                    name: "hasPurchasedHouse",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "ownedHouses",
                    wire: Wire::List {
                        value: &Wire::Seq(&[
                            Wire::Fixed(16),
                            Wire::Fixed(8),
                            Wire::Fixed(8),
                            Wire::Seq(&[
                                Wire::Fixed(16),
                                Wire::Fixed(8),
                                Wire::Fixed(12),
                                Wire::Fixed(8),
                                Wire::Fixed(8),
                                Wire::Fixed(2),
                                Wire::Fixed(1),
                                Wire::Seq(&[
                                    Wire::Array(&Wire::Fixed(4)),
                                    Wire::Fixed(4),
                                    Wire::Fixed(1),
                                    Wire::Fixed(1),
                                    Wire::Fixed(8),
                                    Wire::Fixed(4),
                                ]),
                            ]),
                        ]),
                        key: &Wire::Wide,
                        full: &Wire::Seq(&[
                            Wire::Fixed(16),
                            Wire::Fixed(8),
                            Wire::Fixed(8),
                            Wire::Seq(&[
                                Wire::Fixed(16),
                                Wire::Fixed(8),
                                Wire::Fixed(12),
                                Wire::Fixed(8),
                                Wire::Fixed(8),
                                Wire::Fixed(2),
                                Wire::Fixed(1),
                                Wire::Seq(&[
                                    Wire::Array(&Wire::Fixed(4)),
                                    Wire::Fixed(4),
                                    Wire::Fixed(1),
                                    Wire::Fixed(1),
                                    Wire::Fixed(8),
                                    Wire::Fixed(4),
                                ]),
                            ]),
                        ]),
                    },
                },
                Child {
                    name: "lastHouseRequestResponse",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "m_isWithinAPlot",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "m_isFastTravelChanneling",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "m_phasedHousingPlotEntityId",
                    wire: Wire::Fixed(8),
                },
                Child {
                    name: "m_phasedHouseDataEntityId",
                    wire: Wire::Fixed(8),
                },
                Child {
                    name: "m_phasedHouseOwnerCharacterId",
                    wire: Wire::Text,
                },
                Child {
                    name: "m_debugDataString",
                    wire: Wire::Text,
                },
            ],
            &[],
        ],
    },
    ClassLayout {
        class: 2850,
        name: "MB::AlignToTerrainComponentReplicatedState",
        slots: &[
            &[Child {
                name: "AlignmentMode",
                wire: Wire::Fixed(1),
            }],
            &[],
        ],
    },
    ClassLayout {
        class: 2895,
        name: "MB::MagicComponentReplicatedState",
        slots: &[
            &[
                Child {
                    name: "state",
                    wire: Wire::Fixed(4),
                },
                Child {
                    name: "channel",
                    wire: Wire::Fixed(4),
                },
            ],
            &[],
        ],
    },
    ClassLayout {
        class: 2912,
        name: "MB::SpellComponentReplicatedState",
        slots: &[
            &[
                Child {
                    name: "spllDtId",
                    wire: Wire::Fixed(4),
                },
                Child {
                    name: "strtpos",
                    wire: Wire::Seq(&[Wire::Fixed(4), Wire::Fixed(4), Wire::Fixed(2)]),
                },
                Child {
                    name: "strtrot",
                    wire: Wire::Quat,
                },
                Child {
                    name: "chldpos",
                    wire: Wire::Seq(&[Wire::Fixed(4), Wire::Fixed(4), Wire::Fixed(2)]),
                },
                Child {
                    name: "chldrot",
                    wire: Wire::Quat,
                },
                Child {
                    name: "prevAttch",
                    wire: Wire::Fixed(24),
                },
                Child {
                    name: "attch",
                    wire: Wire::Fixed(24),
                },
                Child {
                    name: "casterId",
                    wire: Wire::Fixed(24),
                },
                Child {
                    name: "spawnTime",
                    wire: Wire::Fixed(8),
                },
                Child {
                    name: "numSpwn",
                    wire: Wire::Fixed(4),
                },
                Child {
                    name: "m_aoeRadiusScaling",
                    wire: Wire::Fixed(4),
                },
            ],
            &[],
        ],
    },
    ClassLayout {
        class: 2913,
        name: "MB::CombatStatusComponentReplicatedState",
        slots: &[
            &[
                Child {
                    name: "replicatedInCombatFlag",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "replicatedInPvpCombatFlag",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "replicatedCombatLoggedOutTimePoint",
                    wire: Wire::Fixed(8),
                },
                Child {
                    name: "replicatedCombatConcludedTimePoint",
                    wire: Wire::Fixed(8),
                },
            ],
            &[],
        ],
    },
    ClassLayout {
        class: 2930,
        name: "MB::InteractReplicatedState",
        slots: &[
            &[
                Child {
                    name: "Enabled",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "HasInteractors",
                    wire: Wire::Fixed(4),
                },
                Child {
                    name: "CooldownUpdates",
                    wire: Wire::List {
                        value: &Wire::Seq(&[Wire::Fixed(4), Wire::Fixed(8)]),
                        key: &Wire::Wide,
                        full: &Wire::Seq(&[Wire::Fixed(4), Wire::Fixed(8)]),
                    },
                },
            ],
            &[],
        ],
    },
    ClassLayout {
        class: 2932,
        name: "MB::CooldownTimersComponentReplicatedState",
        slots: &[
            &[
                Child {
                    name: "cdmap1",
                    wire: Wire::List {
                        value: &Wire::Seq(&[Wire::Fixed(8), Wire::Fixed(8)]),
                        key: &Wire::Fixed(4),
                        full: &Wire::Seq(&[Wire::Fixed(4), Wire::Fixed(8), Wire::Fixed(8)]),
                    },
                },
                Child {
                    name: "cdmap2",
                    wire: Wire::List {
                        value: &Wire::Seq(&[Wire::Fixed(8), Wire::Fixed(8)]),
                        key: &Wire::Fixed(4),
                        full: &Wire::Seq(&[Wire::Fixed(4), Wire::Fixed(8), Wire::Fixed(8)]),
                    },
                },
                Child {
                    name: "cdmap3",
                    wire: Wire::List {
                        value: &Wire::Seq(&[Wire::Fixed(8), Wire::Fixed(8)]),
                        key: &Wire::Fixed(4),
                        full: &Wire::Seq(&[Wire::Fixed(4), Wire::Fixed(8), Wire::Fixed(8)]),
                    },
                },
                Child {
                    name: "ccdmap",
                    wire: Wire::List {
                        value: &Wire::Seq(&[Wire::Fixed(8), Wire::Fixed(4), Wire::Fixed(4)]),
                        key: &Wire::Fixed(4),
                        full: &Wire::Seq(&[
                            Wire::Fixed(4),
                            Wire::Fixed(8),
                            Wire::Fixed(4),
                            Wire::Fixed(4),
                        ]),
                    },
                },
                Child {
                    name: "gencds",
                    wire: Wire::List {
                        value: &Wire::Seq(&[Wire::Fixed(4), Wire::Fixed(4), Wire::Fixed(8)]),
                        key: &Wire::Wide,
                        full: &Wire::Seq(&[Wire::Fixed(4), Wire::Fixed(4), Wire::Fixed(8)]),
                    },
                },
                Child {
                    name: "nextDailyCooldown",
                    wire: Wire::Fixed(8),
                },
                Child {
                    name: "nextWeeklyCooldown",
                    wire: Wire::Fixed(8),
                },
            ],
            &[],
        ],
    },
    ClassLayout {
        class: 2938,
        name: "MB::GlobalStorageComponentReplicatedState",
        slots: &[
            &[
                Child {
                    name: "m_globalItemMap",
                    wire: Wire::List {
                        value: &Wire::Array(&Wire::Seq(&[
                            Wire::Fixed(8),
                            Wire::Prefixed,
                            Wire::Prefixed,
                            Wire::Prefixed,
                            Wire::Prefixed,
                            Wire::Prefixed,
                            Wire::Prefixed,
                            Wire::Prefixed,
                            Wire::Prefixed,
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                            Wire::Fixed(16),
                            Wire::Fixed(1),
                            Wire::Fixed(4),
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                        ])),
                        key: &Wire::Fixed(16),
                        full: &Wire::Seq(&[
                            Wire::Fixed(16),
                            Wire::Array(&Wire::Seq(&[
                                Wire::Fixed(8),
                                Wire::Prefixed,
                                Wire::Prefixed,
                                Wire::Prefixed,
                                Wire::Prefixed,
                                Wire::Prefixed,
                                Wire::Prefixed,
                                Wire::Prefixed,
                                Wire::Prefixed,
                                Wire::Fixed(1),
                                Wire::Fixed(1),
                                Wire::Fixed(1),
                                Wire::Fixed(1),
                                Wire::Fixed(16),
                                Wire::Fixed(1),
                                Wire::Fixed(4),
                                Wire::Fixed(1),
                                Wire::Fixed(1),
                                Wire::Fixed(1),
                                Wire::Fixed(1),
                            ])),
                        ]),
                    },
                },
                Child {
                    name: "m_overflowItemCount",
                    wire: Wire::Fixed(4),
                },
                Child {
                    name: "m_weightMap",
                    wire: Wire::List {
                        value: &Wire::Fixed(4),
                        key: &Wire::Fixed(16),
                        full: &Wire::Seq(&[Wire::Fixed(16), Wire::Fixed(4)]),
                    },
                },
                Child {
                    name: "m_slotCountMap",
                    wire: Wire::List {
                        value: &Wire::Fixed(4),
                        key: &Wire::Fixed(16),
                        full: &Wire::Seq(&[Wire::Fixed(16), Wire::Fixed(4)]),
                    },
                },
            ],
            &[],
        ],
    },
    ClassLayout {
        class: 2947,
        name: "MB::BeamAttackComponentReplicatedState",
        slots: &[
            &[
                Child {
                    name: "beamData",
                    wire: Wire::List {
                        value: &Wire::Seq(&[
                            Wire::Fixed(1),
                            Wire::Fixed(4),
                            Wire::Seq(&[
                                Wire::Fixed(8),
                                Wire::Fixed(12),
                                Wire::Fixed(12),
                                Wire::Fixed(12),
                                Wire::Fixed(12),
                                Wire::Fixed(12),
                                Wire::Array(&Wire::Fixed(8)),
                            ]),
                        ]),
                        key: &Wire::Wide,
                        full: &Wire::Seq(&[
                            Wire::Fixed(1),
                            Wire::Fixed(4),
                            Wire::Seq(&[
                                Wire::Fixed(8),
                                Wire::Fixed(12),
                                Wire::Fixed(12),
                                Wire::Fixed(12),
                                Wire::Fixed(12),
                                Wire::Fixed(12),
                                Wire::Array(&Wire::Fixed(8)),
                            ]),
                        ]),
                    },
                },
                Child {
                    name: "dynamicBeamData",
                    wire: Wire::List {
                        value: &Wire::Seq(&[
                            Wire::Fixed(4),
                            Wire::Seq(&[
                                Wire::Fixed(1),
                                Wire::Fixed(4),
                                Wire::Seq(&[
                                    Wire::Fixed(8),
                                    Wire::Fixed(12),
                                    Wire::Fixed(12),
                                    Wire::Fixed(12),
                                    Wire::Fixed(12),
                                    Wire::Fixed(12),
                                    Wire::Array(&Wire::Fixed(8)),
                                ]),
                            ]),
                        ]),
                        key: &Wire::Wide,
                        full: &Wire::Seq(&[
                            Wire::Fixed(4),
                            Wire::Seq(&[
                                Wire::Fixed(1),
                                Wire::Fixed(4),
                                Wire::Seq(&[
                                    Wire::Fixed(8),
                                    Wire::Fixed(12),
                                    Wire::Fixed(12),
                                    Wire::Fixed(12),
                                    Wire::Fixed(12),
                                    Wire::Fixed(12),
                                    Wire::Array(&Wire::Fixed(8)),
                                ]),
                            ]),
                        ]),
                    },
                },
                Child {
                    name: "aiTargetGDERef",
                    wire: Wire::Fixed(24),
                },
            ],
            &[],
        ],
    },
    ClassLayout {
        class: 3002,
        name: "MB::ItemGenerationComponentReplicatedState",
        slots: &[
            &[
                Child {
                    name: "active",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "itemGenerationTime",
                    wire: Wire::Fixed(8),
                },
            ],
            &[],
        ],
    },
    ClassLayout {
        class: 3055,
        name: "MB::HousingPlotReplicatedState",
        slots: &[
            &[],
            &[Child {
                name: "enterHousingData",
                wire: Wire::Array(&Wire::Seq(&[
                    Wire::Text,
                    Wire::Fixed(4),
                    Wire::Fixed(16),
                    Wire::Fixed(8),
                ])),
            }],
            &[Child {
                name: "purchaseHousingData",
                wire: Wire::Fixed(8),
            }],
            &[],
        ],
    },
    ClassLayout {
        class: 3086,
        name: "MB::LandClaimManagerComponentReplicatedState",
        slots: &[
            &[
                Child {
                    name: "fcpLockTimers",
                    wire: Wire::List {
                        value: &Wire::Fixed(2),
                        key: &Wire::Wide,
                        full: &Wire::Fixed(2),
                    },
                },
                Child {
                    name: "fcpLockAnchor",
                    wire: Wire::Fixed(8),
                },
                Child {
                    name: "replicatedClaimKeys",
                    wire: Wire::List {
                        value: &Wire::Fixed(2),
                        key: &Wire::Wide,
                        full: &Wire::Fixed(2),
                    },
                },
                Child {
                    name: "replicatedConflictFaction",
                    wire: Wire::List {
                        value: &Wire::Fixed(1),
                        key: &Wire::Wide,
                        full: &Wire::Fixed(1),
                    },
                },
                Child {
                    name: "replicatedConflictLotteryEndTime",
                    wire: Wire::List {
                        value: &Wire::Fixed(8),
                        key: &Wire::Wide,
                        full: &Wire::Fixed(8),
                    },
                },
                Child {
                    name: "replicatedConflictStartTime",
                    wire: Wire::List {
                        value: &Wire::Fixed(8),
                        key: &Wire::Wide,
                        full: &Wire::Fixed(8),
                    },
                },
                Child {
                    name: "replicatedDarknessThreshold",
                    wire: Wire::List {
                        value: &Wire::Fixed(4),
                        key: &Wire::Wide,
                        full: &Wire::Fixed(4),
                    },
                },
                Child {
                    name: "replicatedDarknessCycleEndTime",
                    wire: Wire::List {
                        value: &Wire::Fixed(8),
                        key: &Wire::Wide,
                        full: &Wire::Fixed(8),
                    },
                },
                Child {
                    name: "replicatedFactionControlPointData",
                    wire: Wire::List {
                        value: &Wire::Fixed(1),
                        key: &Wire::Wide,
                        full: &Wire::Fixed(1),
                    },
                },
                Child {
                    name: "replicatedFaction1InfluencePercentages",
                    wire: Wire::List {
                        value: &Wire::Fixed(1),
                        key: &Wire::Wide,
                        full: &Wire::Fixed(1),
                    },
                },
                Child {
                    name: "replicatedFaction2InfluencePercentages",
                    wire: Wire::List {
                        value: &Wire::Fixed(1),
                        key: &Wire::Wide,
                        full: &Wire::Fixed(1),
                    },
                },
                Child {
                    name: "replicatedFaction3InfluencePercentages",
                    wire: Wire::List {
                        value: &Wire::Fixed(1),
                        key: &Wire::Wide,
                        full: &Wire::Fixed(1),
                    },
                },
                Child {
                    name: "replicatedGovernance",
                    wire: Wire::List {
                        value: &Wire::Seq(&[
                            Wire::Fixed(4),
                            Wire::Fixed(4),
                            Wire::Fixed(4),
                            Wire::Fixed(4),
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                        ]),
                        key: &Wire::Wide,
                        full: &Wire::Seq(&[
                            Wire::Fixed(4),
                            Wire::Fixed(4),
                            Wire::Fixed(4),
                            Wire::Fixed(4),
                            Wire::Seq(&[Wire::Fixed(1), Wire::Fixed(1)]),
                        ]),
                    },
                },
                Child {
                    name: "replicatedInfluenceRaceStartTime",
                    wire: Wire::List {
                        value: &Wire::Fixed(8),
                        key: &Wire::Wide,
                        full: &Wire::Fixed(8),
                    },
                },
                Child {
                    name: "replicatedPosData",
                    wire: Wire::List {
                        value: &Wire::Fixed(12),
                        key: &Wire::Wide,
                        full: &Wire::Fixed(12),
                    },
                },
                Child {
                    name: "replicatedProgression",
                    wire: Wire::List {
                        value: &Wire::Seq(&[
                            Wire::Array(&Wire::Seq(&[Wire::Fixed(4), Wire::Fixed(1)])),
                            Wire::Array(&Wire::Seq(&[
                                Wire::Fixed(4),
                                Wire::Fixed(1),
                                Wire::Fixed(4),
                            ])),
                            Wire::Fixed(1),
                        ]),
                        key: &Wire::Wide,
                        full: &Wire::Seq(&[
                            Wire::Array(&Wire::Seq(&[Wire::Fixed(4), Wire::Fixed(1)])),
                            Wire::Array(&Wire::Seq(&[
                                Wire::Fixed(4),
                                Wire::Fixed(1),
                                Wire::Fixed(4),
                            ])),
                            Wire::Fixed(1),
                        ]),
                    },
                },
                Child {
                    name: "replicatedOwnerData",
                    wire: Wire::List {
                        value: &Wire::Seq(&[
                            Wire::Fixed(16),
                            Wire::Text,
                            Wire::Fixed(2),
                            Wire::Seq(&[
                                Wire::Fixed(4),
                                Wire::Fixed(4),
                                Wire::Fixed(4),
                                Wire::Fixed(4),
                            ]),
                            Wire::Fixed(2),
                            Wire::Seq(&[
                                Wire::Fixed(4),
                                Wire::Fixed(4),
                                Wire::Fixed(4),
                                Wire::Fixed(4),
                            ]),
                            Wire::Text,
                            Wire::Fixed(1),
                            Wire::Fixed(8),
                        ]),
                        key: &Wire::Wide,
                        full: &Wire::Seq(&[
                            Wire::Fixed(16),
                            Wire::Text,
                            Wire::Fixed(2),
                            Wire::Seq(&[
                                Wire::Fixed(4),
                                Wire::Fixed(4),
                                Wire::Fixed(4),
                                Wire::Fixed(4),
                            ]),
                            Wire::Fixed(2),
                            Wire::Seq(&[
                                Wire::Fixed(4),
                                Wire::Fixed(4),
                                Wire::Fixed(4),
                                Wire::Fixed(4),
                            ]),
                            Wire::Text,
                            Wire::Fixed(1),
                            Wire::Fixed(8),
                        ]),
                    },
                },
                Child {
                    name: "replicatedWarDecThresholdMetFaction",
                    wire: Wire::List {
                        value: &Wire::Fixed(1),
                        key: &Wire::Wide,
                        full: &Wire::Fixed(1),
                    },
                },
            ],
            &[],
        ],
    },
    ClassLayout {
        class: 3105,
        name: "Javelin::DungeonEntranceReplicatedState",
        slots: &[
            &[Child {
                name: "dungeonsSpawned",
                wire: Wire::Array(&Wire::Seq(&[
                    Wire::Seq(&[Wire::Fixed(8), Wire::Fixed(16)]),
                    Wire::Fixed(1),
                    Wire::Fixed(16),
                ])),
            }],
            &[],
        ],
    },
    ClassLayout {
        class: 3133,
        name: "MB::EntitlementComponentReplicatedState",
        slots: &[
            &[
                Child {
                    name: "entitlements",
                    wire: Wire::List {
                        value: &Wire::Fixed(1),
                        key: &Wire::Wide,
                        full: &Wire::Fixed(1),
                    },
                },
                Child {
                    name: "balances",
                    wire: Wire::List {
                        value: &Wire::Fixed(4),
                        key: &Wire::Fixed(4),
                        full: &Wire::Seq(&[Wire::Fixed(4), Wire::Fixed(4)]),
                    },
                },
                Child {
                    name: "entitlementsReceived",
                    wire: Wire::Fixed(1),
                },
            ],
            &[],
        ],
    },
    ClassLayout {
        class: 3139,
        name: "MB::DelayedEventComponentReplicatedState",
        slots: &[
            &[
                Child {
                    name: "totalDelayDuration",
                    wire: Wire::Fixed(2),
                },
                Child {
                    name: "delayedEventCompleted",
                    wire: Wire::Fixed(1),
                },
            ],
            &[],
        ],
    },
    ClassLayout {
        class: 3147,
        name: "Javelin::GuildsComponentReplicatedState",
        slots: &[
            &[
                Child {
                    name: "guildId",
                    wire: Wire::Fixed(16),
                },
                Child {
                    name: "guildRank",
                    wire: Wire::Fixed(2),
                },
            ],
            &[
                Child {
                    name: "usingGmCommands",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "guildForcedRename",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "guildMessageOfTheDay",
                    wire: Wire::Text,
                },
                Child {
                    name: "guildMessageOfTheDayAuthor",
                    wire: Wire::FlagId,
                },
                Child {
                    name: "guildMessageOfTheDayTime",
                    wire: Wire::Fixed(8),
                },
                Child {
                    name: "guildSiegeWindow",
                    wire: Wire::Fixed(4),
                },
                Child {
                    name: "guildLastSiegeWindowSet",
                    wire: Wire::Fixed(8),
                },
                Child {
                    name: "guildTreasuryCurrentFunds",
                    wire: Wire::Fixed(8),
                },
                Child {
                    name: "guildTreasuryDailyWithdrawalLimit",
                    wire: Wire::Fixed(8),
                },
                Child {
                    name: "lastGuildLeaveTime",
                    wire: Wire::Fixed(8),
                },
                Child {
                    name: "guildTreasuryWithdrawnToday",
                    wire: Wire::Fixed(8),
                },
                Child {
                    name: "isGuildMasterSeatOverthrowable",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "isGuildMasterSeatVacant",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "governorLeaveGuildTime",
                    wire: Wire::Fixed(8),
                },
                Child {
                    name: "guildMembersCharacterIdString",
                    wire: Wire::List {
                        value: &Wire::Text,
                        key: &Wire::Wide,
                        full: &Wire::Text,
                    },
                },
                Child {
                    name: "guildMembersRank",
                    wire: Wire::List {
                        value: &Wire::Fixed(2),
                        key: &Wire::Wide,
                        full: &Wire::Fixed(2),
                    },
                },
                Child {
                    name: "guildMembersOnlineStatus",
                    wire: Wire::List {
                        value: &Wire::Fixed(1),
                        key: &Wire::Wide,
                        full: &Wire::Fixed(1),
                    },
                },
                Child {
                    name: "guildMembersLastOnlineTime",
                    wire: Wire::List {
                        value: &Wire::Fixed(8),
                        key: &Wire::Wide,
                        full: &Wire::Fixed(8),
                    },
                },
                Child {
                    name: "guildMemberWasKicked",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "guildInfluenceData",
                    wire: Wire::List {
                        value: &Wire::Seq(&[Wire::Fixed(2), Wire::Fixed(4), Wire::Fixed(8)]),
                        key: &Wire::Wide,
                        full: &Wire::Seq(&[Wire::Fixed(2), Wire::Fixed(4), Wire::Fixed(8)]),
                    },
                },
                Child {
                    name: "eligibleTerritoryWars",
                    wire: Wire::List {
                        value: &Wire::Seq(&[Wire::Fixed(8), Wire::Fixed(1), Wire::Fixed(2)]),
                        key: &Wire::Wide,
                        full: &Wire::Seq(&[Wire::Fixed(8), Wire::Fixed(1), Wire::Fixed(2)]),
                    },
                },
                Child {
                    name: "guildWarLotteryDeadline",
                    wire: Wire::Fixed(8),
                },
                Child {
                    name: "guildInvites",
                    wire: Wire::List {
                        value: &Wire::Seq(&[
                            Wire::Text,
                            Wire::Seq(&[
                                Wire::Fixed(2),
                                Wire::Seq(&[
                                    Wire::Fixed(4),
                                    Wire::Fixed(4),
                                    Wire::Fixed(4),
                                    Wire::Fixed(4),
                                ]),
                                Wire::Fixed(2),
                                Wire::Seq(&[
                                    Wire::Fixed(4),
                                    Wire::Fixed(4),
                                    Wire::Fixed(4),
                                    Wire::Fixed(4),
                                ]),
                            ]),
                            Wire::Text,
                            Wire::Seq(&[Wire::Fixed(4), Wire::Text]),
                            Wire::Fixed(8),
                        ]),
                        key: &Wire::Fixed(16),
                        full: &Wire::Seq(&[
                            Wire::Fixed(16),
                            Wire::Seq(&[
                                Wire::Text,
                                Wire::Seq(&[
                                    Wire::Fixed(2),
                                    Wire::Seq(&[
                                        Wire::Fixed(4),
                                        Wire::Fixed(4),
                                        Wire::Fixed(4),
                                        Wire::Fixed(4),
                                    ]),
                                    Wire::Fixed(2),
                                    Wire::Seq(&[
                                        Wire::Fixed(4),
                                        Wire::Fixed(4),
                                        Wire::Fixed(4),
                                        Wire::Fixed(4),
                                    ]),
                                ]),
                                Wire::Text,
                                Wire::Seq(&[Wire::Fixed(4), Wire::Text]),
                                Wire::Fixed(8),
                            ]),
                        ]),
                    },
                },
                Child {
                    name: "numberOfOutstandingInvites",
                    wire: Wire::Fixed(8),
                },
            ],
            &[],
        ],
    },
    ClassLayout {
        class: 3152,
        name: "MB::FactionComponentReplicatedState",
        slots: &[
            &[],
            &[
                Child {
                    name: "pvpFlagPending",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "notifyPending",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "pvpFlagPendingEndTime",
                    wire: Wire::Fixed(8),
                },
                Child {
                    name: "lastFactionChangeTimepoint",
                    wire: Wire::Fixed(8),
                },
                Child {
                    name: "factionChangeCount",
                    wire: Wire::Fixed(2),
                },
                Child {
                    name: "ffaPendingEndTime",
                    wire: Wire::Fixed(8),
                },
                Child {
                    name: "ffaAntiGroupingIsCursing",
                    wire: Wire::Fixed(1),
                },
            ],
            &[
                Child {
                    name: "faction",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "pvpFlag",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "hasSanctuary",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "ffaFlag",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "ffaAntiGroupingCurseStacks",
                    wire: Wire::Fixed(1),
                },
            ],
            &[
                Child {
                    name: "timeAtFlagStart",
                    wire: Wire::Fixed(8),
                },
                Child {
                    name: "m_pvpValue",
                    wire: Wire::Fixed(4),
                },
                Child {
                    name: "isAccumulatingPvpValue",
                    wire: Wire::Fixed(1),
                },
            ],
            &[],
        ],
    },
    ClassLayout {
        class: 3183,
        name: "MB::PaperdollComponentReplicatedState",
        slots: &[
            &[],
            &[
                Child {
                    name: "visibleDurability",
                    wire: Wire::List {
                        value: &Wire::Fixed(4),
                        key: &Wire::Wide,
                        full: &Wire::Fixed(4),
                    },
                },
                Child {
                    name: "visibleFullItemData",
                    wire: Wire::List {
                        value: &Wire::Seq(&[
                            Wire::Fixed(8),
                            Wire::Prefixed,
                            Wire::Prefixed,
                            Wire::Prefixed,
                            Wire::Prefixed,
                            Wire::Prefixed,
                            Wire::Prefixed,
                            Wire::Prefixed,
                            Wire::Prefixed,
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                            Wire::Fixed(16),
                            Wire::Fixed(1),
                            Wire::Fixed(4),
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                        ]),
                        key: &Wire::Wide,
                        full: &Wire::Seq(&[
                            Wire::Fixed(8),
                            Wire::Prefixed,
                            Wire::Prefixed,
                            Wire::Prefixed,
                            Wire::Prefixed,
                            Wire::Prefixed,
                            Wire::Prefixed,
                            Wire::Prefixed,
                            Wire::Prefixed,
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                            Wire::Fixed(16),
                            Wire::Fixed(1),
                            Wire::Fixed(4),
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                        ]),
                    },
                },
                Child {
                    name: "nonVisibleFullItemData",
                    wire: Wire::List {
                        value: &Wire::Seq(&[
                            Wire::Fixed(8),
                            Wire::Prefixed,
                            Wire::Prefixed,
                            Wire::Prefixed,
                            Wire::Prefixed,
                            Wire::Prefixed,
                            Wire::Prefixed,
                            Wire::Prefixed,
                            Wire::Prefixed,
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                            Wire::Fixed(16),
                            Wire::Fixed(1),
                            Wire::Fixed(4),
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                        ]),
                        key: &Wire::Wide,
                        full: &Wire::Seq(&[
                            Wire::Fixed(8),
                            Wire::Prefixed,
                            Wire::Prefixed,
                            Wire::Prefixed,
                            Wire::Prefixed,
                            Wire::Prefixed,
                            Wire::Prefixed,
                            Wire::Prefixed,
                            Wire::Prefixed,
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                            Wire::Fixed(16),
                            Wire::Fixed(1),
                            Wire::Fixed(4),
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                        ]),
                    },
                },
                Child {
                    name: "nonVisiblePaperdollSlots",
                    wire: Wire::List {
                        value: &Wire::Fixed(4),
                        key: &Wire::Wide,
                        full: &Wire::Fixed(4),
                    },
                },
                Child {
                    name: "nonVisibleDurability",
                    wire: Wire::List {
                        value: &Wire::Fixed(4),
                        key: &Wire::Wide,
                        full: &Wire::Fixed(4),
                    },
                },
                Child {
                    name: "bonusEquipLoad",
                    wire: Wire::Fixed(2),
                },
                Child {
                    name: "isLocalPlayer",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "isLoadoutPanelOpen",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "mainHandOption1LoadedAmmoData",
                    wire: Wire::Seq(&[Wire::Fixed(1), Wire::Fixed(8)]),
                },
                Child {
                    name: "mainHandOption2LoadedAmmoData",
                    wire: Wire::Seq(&[Wire::Fixed(1), Wire::Fixed(8)]),
                },
                Child {
                    name: "loadouts",
                    wire: Wire::List {
                        value: &Wire::Seq(&[
                            Wire::Text,
                            Wire::Seq(&[
                                Wire::Array(&Wire::Seq(&[Wire::Fixed(4), Wire::Fixed(2)])),
                                Wire::Fixed(1),
                                Wire::Array(&Wire::Seq(&[Wire::Fixed(4), Wire::Fixed(4)])),
                            ]),
                        ]),
                        key: &Wire::Wide,
                        full: &Wire::Seq(&[
                            Wire::Text,
                            Wire::Seq(&[
                                Wire::Array(&Wire::Seq(&[Wire::Fixed(4), Wire::Fixed(2)])),
                                Wire::Fixed(1),
                                Wire::Array(&Wire::Seq(&[Wire::Fixed(4), Wire::Fixed(4)])),
                            ]),
                        ]),
                    },
                },
                Child {
                    name: "hideSkins",
                    wire: Wire::List {
                        value: &Wire::Fixed(1),
                        key: &Wire::Fixed(4),
                        full: &Wire::Seq(&[Wire::Fixed(4), Wire::Fixed(1)]),
                    },
                },
            ],
            &[
                Child {
                    name: "sheatheMap",
                    wire: Wire::Fixed(7),
                },
                Child {
                    name: "itemSlotsAttachmentStatus",
                    wire: Wire::Fixed(7),
                },
                Child {
                    name: "activeMap",
                    wire: Wire::Fixed(7),
                },
                Child {
                    name: "visiblePaperdollSlots",
                    wire: Wire::List {
                        value: &Wire::Fixed(4),
                        key: &Wire::Wide,
                        full: &Wire::Fixed(4),
                    },
                },
                Child {
                    name: "visibleItemVisualData",
                    wire: Wire::List {
                        value: &Wire::Seq(&[
                            Wire::Prefixed,
                            Wire::Fixed(1),
                            Wire::Prefixed,
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                            Wire::Fixed(4),
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                        ]),
                        key: &Wire::Wide,
                        full: &Wire::Seq(&[
                            Wire::Prefixed,
                            Wire::Fixed(1),
                            Wire::Prefixed,
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                            Wire::Fixed(4),
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                        ]),
                    },
                },
                Child {
                    name: "loadoutSwapIncrement",
                    wire: Wire::Fixed(1),
                },
            ],
            &[],
        ],
    },
    ClassLayout {
        class: 3194,
        name: "MB::FtueIslandComponentReplicatedState",
        slots: &[
            &[
                Child {
                    name: "playerGde",
                    wire: Wire::Fixed(24),
                },
                Child {
                    name: "regionX",
                    wire: Wire::Prefixed,
                },
                Child {
                    name: "regionY",
                    wire: Wire::Prefixed,
                },
                Child {
                    name: "regionSize",
                    wire: Wire::Prefixed,
                },
            ],
            &[],
        ],
    },
    ClassLayout {
        class: 3204,
        name: "MB::VoidDestroyerComponentReplicatedState",
        slots: &[
            &[
                Child {
                    name: "leftEyeGDERef",
                    wire: Wire::Fixed(24),
                },
                Child {
                    name: "rightEyeGDERef",
                    wire: Wire::Fixed(24),
                },
            ],
            &[],
        ],
    },
    ClassLayout {
        class: 3217,
        name: "MB::OwnershipComponentReplicatedState",
        slots: &[
            &[
                Child {
                    name: "guildID",
                    wire: Wire::Fixed(16),
                },
                Child {
                    name: "guildOwner",
                    wire: Wire::Text,
                },
                Child {
                    name: "guildCrestData",
                    wire: Wire::Seq(&[
                        Wire::Fixed(2),
                        Wire::Seq(&[
                            Wire::Fixed(4),
                            Wire::Fixed(4),
                            Wire::Fixed(4),
                            Wire::Fixed(4),
                        ]),
                        Wire::Fixed(2),
                        Wire::Seq(&[
                            Wire::Fixed(4),
                            Wire::Fixed(4),
                            Wire::Fixed(4),
                            Wire::Fixed(4),
                        ]),
                    ]),
                },
                Child {
                    name: "ownerPlayerIdentification",
                    wire: Wire::Seq(&[Wire::Text, Wire::Text, Wire::Fixed(1)]),
                },
                Child {
                    name: "currentPermissionRule",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "guildStructureName",
                    wire: Wire::Text,
                },
                Child {
                    name: "raidId",
                    wire: Wire::Fixed(8),
                },
                Child {
                    name: "factionType",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "ownerPvpFlag",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "ownerFFAFlag",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "permissionOptions",
                    wire: Wire::Fixed(1),
                },
            ],
            &[],
        ],
    },
    ClassLayout {
        class: 3290,
        name: "MB::PlayerTimeComponentReplicatedState",
        slots: &[
            &[
                Child {
                    name: "startTimePoint",
                    wire: Wire::Fixed(8),
                },
                Child {
                    name: "durationAtStart",
                    wire: Wire::Fixed(8),
                },
                Child {
                    name: "paidDurationAtStart",
                    wire: Wire::Fixed(8),
                },
            ],
            &[],
        ],
    },
    ClassLayout {
        class: 3312,
        name: "Javelin::GameModeParticipantReplicatedState",
        slots: &[
            &[
                Child {
                    name: "activeGameModes",
                    wire: Wire::List {
                        value: &Wire::Seq(&[
                            Wire::Fixed(4),
                            Wire::Fixed(4),
                            Wire::Fixed(8),
                            Wire::Fixed(8),
                        ]),
                        key: &Wire::Seq(&[Wire::Fixed(16), Wire::Fixed(8), Wire::Fixed(8)]),
                        full: &Wire::Seq(&[
                            Wire::Fixed(16),
                            Wire::Fixed(8),
                            Wire::Fixed(8),
                            Wire::Seq(&[
                                Wire::Fixed(4),
                                Wire::Fixed(4),
                                Wire::Fixed(8),
                                Wire::Fixed(8),
                            ]),
                        ]),
                    },
                },
                Child {
                    name: "flags",
                    wire: Wire::Fixed(6),
                },
                Child {
                    name: "queuingForGameModes",
                    wire: Wire::List {
                        value: &Wire::Seq(&[
                            Wire::Fixed(4),
                            Wire::Fixed(8),
                            Wire::Fixed(4),
                            Wire::Fixed(4),
                            Wire::Fixed(1),
                        ]),
                        key: &Wire::Wide,
                        full: &Wire::Seq(&[
                            Wire::Fixed(4),
                            Wire::Fixed(8),
                            Wire::Fixed(4),
                            Wire::Fixed(4),
                            Wire::Fixed(1),
                        ]),
                    },
                },
                Child {
                    name: "queueEligibleTimesForGameModes",
                    wire: Wire::List {
                        value: &Wire::Fixed(8),
                        key: &Wire::Fixed(4),
                        full: &Wire::Seq(&[Wire::Fixed(4), Wire::Fixed(8)]),
                    },
                },
                Child {
                    name: "gameModeMutationContext",
                    wire: Wire::Seq(&[
                        Wire::Fixed(4),
                        Wire::Fixed(4),
                        Wire::Fixed(4),
                        Wire::Fixed(1),
                    ]),
                },
                Child {
                    name: "lastTeamIndex",
                    wire: Wire::Fixed(1),
                },
            ],
            &[
                Child {
                    name: "matchmakingServiceActivity",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "matchmakingServiceStatus",
                    wire: Wire::Fixed(4),
                },
                Child {
                    name: "matchmakingServiceMatchId",
                    wire: Wire::Text,
                },
                Child {
                    name: "matchmakingServiceDesiredPlayers",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "matchmakingServieAcceptedPlayers",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "isReplicatingGroupActivity",
                    wire: Wire::Fixed(1),
                },
            ],
            &[Child {
                name: "groupActivityEligibility",
                wire: Wire::Fixed(16),
            }],
            &[],
        ],
    },
    ClassLayout {
        class: 3362,
        name: "MB::SlayerScriptReplicatedState",
        slots: &[
            &[
                Child {
                    name: "curScriptStateId",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "curScriptId",
                    wire: Wire::Fixed(4),
                },
                Child {
                    name: "spawnedEntityIdsBySpawnerId",
                    wire: Wire::List {
                        value: &Wire::Fixed(8),
                        key: &Wire::Fixed(4),
                        full: &Wire::Seq(&[Wire::Fixed(4), Wire::Fixed(8)]),
                    },
                },
            ],
            &[],
        ],
    },
    ClassLayout {
        class: 3408,
        name: "Javelin::PlayerGenericInviteReplicatedState",
        slots: &[
            &[Child {
                name: "m_inviteId",
                wire: Wire::Fixed(16),
            }],
            &[
                Child {
                    name: "m_activityCrc",
                    wire: Wire::Fixed(4),
                },
                Child {
                    name: "m_forwardType",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "m_inviteParticipants",
                    wire: Wire::Seq(&[
                        Wire::Array(&Wire::Seq(&[
                            Wire::FlagId,
                            Wire::Seq(&[Wire::Fixed(4), Wire::Text]),
                        ])),
                        Wire::Seq(&[
                            Wire::Array(&Wire::FlagId),
                            Wire::Array(&Wire::Seq(&[Wire::FlagId, Wire::Fixed(4)])),
                        ]),
                    ]),
                },
                Child {
                    name: "m_expiryTimePoint",
                    wire: Wire::Fixed(8),
                },
            ],
            &[],
        ],
    },
    ClassLayout {
        class: 3451,
        name: "Javelin::GroupDataComponentReplicatedState",
        slots: &[
            &[
                Child {
                    name: "groupLeaderWorldPositionRelative",
                    wire: Wire::Seq(&[Wire::Fixed(1), Wire::Fixed(1), Wire::Fixed(1)]),
                },
                Child {
                    name: "groupLeaderWorldPositionAbsolute",
                    wire: Wire::Seq(&[Wire::Fixed(4), Wire::Fixed(4), Wire::Fixed(2)]),
                },
                Child {
                    name: "groupMemberRole",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "groupMemberMatchmakingRole",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "groupMemberHealth",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "groupMemberMana",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "groupMemberFlags",
                    wire: Wire::Fixed(2),
                },
                Child {
                    name: "groupMemberRole",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "groupMemberMatchmakingRole",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "groupMemberHealth",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "groupMemberMana",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "groupMemberFlags",
                    wire: Wire::Fixed(2),
                },
                Child {
                    name: "groupMemberRole",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "groupMemberMatchmakingRole",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "groupMemberHealth",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "groupMemberMana",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "groupMemberFlags",
                    wire: Wire::Fixed(2),
                },
                Child {
                    name: "groupMemberRole",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "groupMemberMatchmakingRole",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "groupMemberHealth",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "groupMemberMana",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "groupMemberFlags",
                    wire: Wire::Fixed(2),
                },
                Child {
                    name: "groupMemberRole",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "groupMemberMatchmakingRole",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "groupMemberHealth",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "groupMemberMana",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "groupMemberFlags",
                    wire: Wire::Fixed(2),
                },
                Child {
                    name: "groupLeaderPingData",
                    wire: Wire::Seq(&[
                        Wire::Seq(&[
                            Wire::Fixed(1),
                            Wire::Fixed(12),
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                        ]),
                        Wire::Fixed(1),
                    ]),
                },
                Child {
                    name: "raidMemberPosition",
                    wire: Wire::Seq(&[Wire::Fixed(2), Wire::Fixed(2), Wire::Fixed(2)]),
                },
                Child {
                    name: "raidMemberPosition",
                    wire: Wire::Seq(&[Wire::Fixed(2), Wire::Fixed(2), Wire::Fixed(2)]),
                },
                Child {
                    name: "raidMemberPosition",
                    wire: Wire::Seq(&[Wire::Fixed(2), Wire::Fixed(2), Wire::Fixed(2)]),
                },
                Child {
                    name: "raidMemberPosition",
                    wire: Wire::Seq(&[Wire::Fixed(2), Wire::Fixed(2), Wire::Fixed(2)]),
                },
                Child {
                    name: "raidMemberPosition",
                    wire: Wire::Seq(&[Wire::Fixed(2), Wire::Fixed(2), Wire::Fixed(2)]),
                },
                Child {
                    name: "groupMemberBaseData",
                    wire: Wire::Seq(&[
                        Wire::FlagId,
                        Wire::Text,
                        Wire::Seq(&[
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                            Wire::Fixed(2),
                        ]),
                    ]),
                },
                Child {
                    name: "groupMemberBaseData",
                    wire: Wire::Seq(&[
                        Wire::FlagId,
                        Wire::Text,
                        Wire::Seq(&[
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                            Wire::Fixed(2),
                        ]),
                    ]),
                },
                Child {
                    name: "groupMemberBaseData",
                    wire: Wire::Seq(&[
                        Wire::FlagId,
                        Wire::Text,
                        Wire::Seq(&[
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                            Wire::Fixed(2),
                        ]),
                    ]),
                },
                Child {
                    name: "groupMemberBaseData",
                    wire: Wire::Seq(&[
                        Wire::FlagId,
                        Wire::Text,
                        Wire::Seq(&[
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                            Wire::Fixed(2),
                        ]),
                    ]),
                },
                Child {
                    name: "groupMemberBaseData",
                    wire: Wire::Seq(&[
                        Wire::FlagId,
                        Wire::Text,
                        Wire::Seq(&[
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                            Wire::Fixed(2),
                        ]),
                    ]),
                },
                Child {
                    name: "groupId",
                    wire: Wire::Fixed(16),
                },
                Child {
                    name: "groupDungeonTime",
                    wire: Wire::Fixed(8),
                },
                Child {
                    name: "groupRaidGroupIndexField",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "groupNameIndex",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "groupIconIndex",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "groupColorIndex",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "groupObjectiveIndex",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "groupDungeonState",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "groupGameModeIdx",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "groupGameModeDifficulty",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "joinQueueThrottleActive",
                    wire: Wire::Fixed(1),
                },
            ],
            &[
                Child {
                    name: "groupMemberWorldPositionRelative",
                    wire: Wire::Seq(&[Wire::Fixed(1), Wire::Fixed(1), Wire::Fixed(1)]),
                },
                Child {
                    name: "groupMemberWorldPositionAbsolute",
                    wire: Wire::Seq(&[Wire::Fixed(4), Wire::Fixed(4), Wire::Fixed(2)]),
                },
                Child {
                    name: "groupMemberGameModeIndex",
                    wire: Wire::Seq(&[Wire::Fixed(1), Wire::Fixed(1)]),
                },
                Child {
                    name: "groupMemberFaction",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "groupMemberLevel",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "groupMemberWaypointPosition",
                    wire: Wire::Fixed(8),
                },
                Child {
                    name: "groupMemberPingData",
                    wire: Wire::Seq(&[
                        Wire::Fixed(1),
                        Wire::Fixed(12),
                        Wire::Fixed(1),
                        Wire::Fixed(1),
                    ]),
                },
                Child {
                    name: "groupMemberPingTargetPosition",
                    wire: Wire::Seq(&[Wire::Fixed(8), Wire::Fixed(1)]),
                },
                Child {
                    name: "groupMemberHouseIds",
                    wire: Wire::List {
                        value: &Wire::Seq(&[
                            Wire::Fixed(16),
                            Wire::Fixed(8),
                            Wire::Fixed(8),
                            Wire::Seq(&[Wire::Fixed(16), Wire::Fixed(8)]),
                        ]),
                        key: &Wire::Wide,
                        full: &Wire::Seq(&[
                            Wire::Fixed(16),
                            Wire::Fixed(8),
                            Wire::Fixed(8),
                            Wire::Seq(&[Wire::Fixed(16), Wire::Fixed(8)]),
                        ]),
                    },
                },
            ],
            &[
                Child {
                    name: "groupMemberWorldPositionRelative",
                    wire: Wire::Seq(&[Wire::Fixed(1), Wire::Fixed(1), Wire::Fixed(1)]),
                },
                Child {
                    name: "groupMemberWorldPositionAbsolute",
                    wire: Wire::Seq(&[Wire::Fixed(4), Wire::Fixed(4), Wire::Fixed(2)]),
                },
                Child {
                    name: "groupMemberGameModeIndex",
                    wire: Wire::Seq(&[Wire::Fixed(1), Wire::Fixed(1)]),
                },
                Child {
                    name: "groupMemberFaction",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "groupMemberLevel",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "groupMemberWaypointPosition",
                    wire: Wire::Fixed(8),
                },
                Child {
                    name: "groupMemberPingData",
                    wire: Wire::Seq(&[
                        Wire::Fixed(1),
                        Wire::Fixed(12),
                        Wire::Fixed(1),
                        Wire::Fixed(1),
                    ]),
                },
                Child {
                    name: "groupMemberPingTargetPosition",
                    wire: Wire::Seq(&[Wire::Fixed(8), Wire::Fixed(1)]),
                },
                Child {
                    name: "groupMemberHouseIds",
                    wire: Wire::List {
                        value: &Wire::Seq(&[
                            Wire::Fixed(16),
                            Wire::Fixed(8),
                            Wire::Fixed(8),
                            Wire::Seq(&[Wire::Fixed(16), Wire::Fixed(8)]),
                        ]),
                        key: &Wire::Wide,
                        full: &Wire::Seq(&[
                            Wire::Fixed(16),
                            Wire::Fixed(8),
                            Wire::Fixed(8),
                            Wire::Seq(&[Wire::Fixed(16), Wire::Fixed(8)]),
                        ]),
                    },
                },
            ],
            &[
                Child {
                    name: "groupMemberWorldPositionRelative",
                    wire: Wire::Seq(&[Wire::Fixed(1), Wire::Fixed(1), Wire::Fixed(1)]),
                },
                Child {
                    name: "groupMemberWorldPositionAbsolute",
                    wire: Wire::Seq(&[Wire::Fixed(4), Wire::Fixed(4), Wire::Fixed(2)]),
                },
                Child {
                    name: "groupMemberGameModeIndex",
                    wire: Wire::Seq(&[Wire::Fixed(1), Wire::Fixed(1)]),
                },
                Child {
                    name: "groupMemberFaction",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "groupMemberLevel",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "groupMemberWaypointPosition",
                    wire: Wire::Fixed(8),
                },
                Child {
                    name: "groupMemberPingData",
                    wire: Wire::Seq(&[
                        Wire::Fixed(1),
                        Wire::Fixed(12),
                        Wire::Fixed(1),
                        Wire::Fixed(1),
                    ]),
                },
                Child {
                    name: "groupMemberPingTargetPosition",
                    wire: Wire::Seq(&[Wire::Fixed(8), Wire::Fixed(1)]),
                },
                Child {
                    name: "groupMemberHouseIds",
                    wire: Wire::List {
                        value: &Wire::Seq(&[
                            Wire::Fixed(16),
                            Wire::Fixed(8),
                            Wire::Fixed(8),
                            Wire::Seq(&[Wire::Fixed(16), Wire::Fixed(8)]),
                        ]),
                        key: &Wire::Wide,
                        full: &Wire::Seq(&[
                            Wire::Fixed(16),
                            Wire::Fixed(8),
                            Wire::Fixed(8),
                            Wire::Seq(&[Wire::Fixed(16), Wire::Fixed(8)]),
                        ]),
                    },
                },
            ],
            &[
                Child {
                    name: "groupMemberWorldPositionRelative",
                    wire: Wire::Seq(&[Wire::Fixed(1), Wire::Fixed(1), Wire::Fixed(1)]),
                },
                Child {
                    name: "groupMemberWorldPositionAbsolute",
                    wire: Wire::Seq(&[Wire::Fixed(4), Wire::Fixed(4), Wire::Fixed(2)]),
                },
                Child {
                    name: "groupMemberGameModeIndex",
                    wire: Wire::Seq(&[Wire::Fixed(1), Wire::Fixed(1)]),
                },
                Child {
                    name: "groupMemberFaction",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "groupMemberLevel",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "groupMemberWaypointPosition",
                    wire: Wire::Fixed(8),
                },
                Child {
                    name: "groupMemberPingData",
                    wire: Wire::Seq(&[
                        Wire::Fixed(1),
                        Wire::Fixed(12),
                        Wire::Fixed(1),
                        Wire::Fixed(1),
                    ]),
                },
                Child {
                    name: "groupMemberPingTargetPosition",
                    wire: Wire::Seq(&[Wire::Fixed(8), Wire::Fixed(1)]),
                },
                Child {
                    name: "groupMemberHouseIds",
                    wire: Wire::List {
                        value: &Wire::Seq(&[
                            Wire::Fixed(16),
                            Wire::Fixed(8),
                            Wire::Fixed(8),
                            Wire::Seq(&[Wire::Fixed(16), Wire::Fixed(8)]),
                        ]),
                        key: &Wire::Wide,
                        full: &Wire::Seq(&[
                            Wire::Fixed(16),
                            Wire::Fixed(8),
                            Wire::Fixed(8),
                            Wire::Seq(&[Wire::Fixed(16), Wire::Fixed(8)]),
                        ]),
                    },
                },
            ],
            &[
                Child {
                    name: "groupMemberWorldPositionRelative",
                    wire: Wire::Seq(&[Wire::Fixed(1), Wire::Fixed(1), Wire::Fixed(1)]),
                },
                Child {
                    name: "groupMemberWorldPositionAbsolute",
                    wire: Wire::Seq(&[Wire::Fixed(4), Wire::Fixed(4), Wire::Fixed(2)]),
                },
                Child {
                    name: "groupMemberGameModeIndex",
                    wire: Wire::Seq(&[Wire::Fixed(1), Wire::Fixed(1)]),
                },
                Child {
                    name: "groupMemberFaction",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "groupMemberLevel",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "groupMemberWaypointPosition",
                    wire: Wire::Fixed(8),
                },
                Child {
                    name: "groupMemberPingData",
                    wire: Wire::Seq(&[
                        Wire::Fixed(1),
                        Wire::Fixed(12),
                        Wire::Fixed(1),
                        Wire::Fixed(1),
                    ]),
                },
                Child {
                    name: "groupMemberPingTargetPosition",
                    wire: Wire::Seq(&[Wire::Fixed(8), Wire::Fixed(1)]),
                },
                Child {
                    name: "groupMemberHouseIds",
                    wire: Wire::List {
                        value: &Wire::Seq(&[
                            Wire::Fixed(16),
                            Wire::Fixed(8),
                            Wire::Fixed(8),
                            Wire::Seq(&[Wire::Fixed(16), Wire::Fixed(8)]),
                        ]),
                        key: &Wire::Wide,
                        full: &Wire::Seq(&[
                            Wire::Fixed(16),
                            Wire::Fixed(8),
                            Wire::Fixed(8),
                            Wire::Seq(&[Wire::Fixed(16), Wire::Fixed(8)]),
                        ]),
                    },
                },
            ],
            &[],
        ],
    },
    ClassLayout {
        class: 3652,
        name: "MB::PlayerHomeComponentReplicatedState",
        slots: &[
            &[
                Child {
                    name: "homePointList",
                    wire: Wire::List {
                        value: &Wire::Seq(&[
                            Wire::Fixed(16),
                            Wire::Fixed(8),
                            Wire::Fixed(8),
                            Wire::Text,
                            Wire::Seq(&[
                                Wire::Fixed(12),
                                Wire::Fixed(8),
                                Wire::Fixed(8),
                                Wire::Fixed(4),
                                Wire::Fixed(1),
                                Wire::Fixed(1),
                                Wire::Text,
                                Wire::Fixed(4),
                            ]),
                        ]),
                        key: &Wire::Wide,
                        full: &Wire::Seq(&[
                            Wire::Fixed(16),
                            Wire::Fixed(8),
                            Wire::Fixed(8),
                            Wire::Text,
                            Wire::Seq(&[
                                Wire::Fixed(12),
                                Wire::Fixed(8),
                                Wire::Fixed(8),
                                Wire::Fixed(4),
                                Wire::Fixed(1),
                                Wire::Fixed(1),
                                Wire::Text,
                                Wire::Fixed(4),
                            ]),
                        ]),
                    },
                },
                Child {
                    name: "homePointId",
                    wire: Wire::Text,
                },
            ],
            &[],
        ],
    },
    ClassLayout {
        class: 3663,
        name: "Javelin::HouseDataReplicatedState",
        slots: &[
            &[Child {
                name: "housingItems",
                wire: Wire::List {
                    value: &Wire::Seq(&[
                        Wire::Fixed(2),
                        Wire::Fixed(2),
                        Wire::Fixed(2),
                        Wire::PackedTurn,
                        Wire::Fixed(4),
                        Wire::Fixed(1),
                    ]),
                    key: &Wire::Wide,
                    full: &Wire::Seq(&[
                        Wire::Fixed(2),
                        Wire::Fixed(2),
                        Wire::Fixed(2),
                        Wire::PackedTurn,
                        Wire::Fixed(4),
                        Wire::Fixed(1),
                    ]),
                },
            }],
            &[],
        ],
    },
    ClassLayout {
        class: 3681,
        name: "MB::ProgressionPointReplicatedState",
        slots: &[
            &[
                Child {
                    name: "poolIds",
                    wire: Wire::List {
                        value: &Wire::Fixed(4),
                        key: &Wire::Wide,
                        full: &Wire::Fixed(4),
                    },
                },
                Child {
                    name: "poolCounts",
                    wire: Wire::List {
                        value: &Wire::Fixed(2),
                        key: &Wire::Wide,
                        full: &Wire::Fixed(2),
                    },
                },
                Child {
                    name: "poolCategories",
                    wire: Wire::List {
                        value: &Wire::Fixed(1),
                        key: &Wire::Wide,
                        full: &Wire::Fixed(1),
                    },
                },
                Child {
                    name: "pointEntries",
                    wire: Wire::List {
                        value: &Wire::Seq(&[Wire::Fixed(4), Wire::Fixed(4), Wire::Fixed(2)]),
                        key: &Wire::Wide,
                        full: &Wire::Seq(&[Wire::Fixed(4), Wire::Fixed(4), Wire::Fixed(2)]),
                    },
                },
            ],
            &[],
        ],
    },
    ClassLayout {
        class: 3752,
        name: "MB::InteractorComponentReplicatedState",
        slots: &[
            &[Child {
                name: "m_cachedCommittedInteractGDEID",
                wire: Wire::Fixed(8),
            }],
            &[Child {
                name: "m_enabled",
                wire: Wire::Fixed(1),
            }],
            &[],
        ],
    },
    ClassLayout {
        class: 3765,
        name: "MB::ItemSkinningComponentReplicatedState",
        slots: &[
            &[],
            &[
                Child {
                    name: "m_enabledItemSkins",
                    wire: Wire::List {
                        value: &Wire::Fixed(8),
                        key: &Wire::Wide,
                        full: &Wire::Fixed(8),
                    },
                },
                Child {
                    name: "m_skinDyeData",
                    wire: Wire::List {
                        value: &Wire::Seq(&[
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                        ]),
                        key: &Wire::Fixed(4),
                        full: &Wire::Seq(&[
                            Wire::Fixed(4),
                            Wire::Seq(&[
                                Wire::Fixed(1),
                                Wire::Fixed(1),
                                Wire::Fixed(1),
                                Wire::Fixed(1),
                            ]),
                        ]),
                    },
                },
            ],
            &[],
        ],
    },
    ClassLayout {
        class: 3780,
        name: "Javelin::GlobalMapDataManagerComponentReplicatedState",
        slots: &[
            &[Child {
                name: "globalMapData",
                wire: Wire::List {
                    value: &Wire::Seq(&[
                        Wire::Fixed(8),
                        Wire::Fixed(2),
                        Wire::Fixed(4),
                        Wire::Fixed(1),
                    ]),
                    key: &Wire::Fixed(8),
                    full: &Wire::Seq(&[
                        Wire::Fixed(8),
                        Wire::Seq(&[
                            Wire::Fixed(8),
                            Wire::Fixed(2),
                            Wire::Fixed(4),
                            Wire::Fixed(1),
                        ]),
                    ]),
                },
            }],
            &[],
        ],
    },
    ClassLayout {
        class: 3786,
        name: "MB::CurrencyComponentReplicatedState",
        slots: &[
            &[Child {
                name: "currency",
                wire: Wire::Fixed(8),
            }],
            &[],
        ],
    },
    ClassLayout {
        class: 3791,
        name: "MB::TradingPostComponentReplicatedState",
        slots: &[
            &[
                Child {
                    name: "totalBuyContracts",
                    wire: Wire::Fixed(4),
                },
                Child {
                    name: "totalSellContracts",
                    wire: Wire::Fixed(4),
                },
            ],
            &[
                Child {
                    name: "buyCategoryCounts",
                    wire: Wire::List {
                        value: &Wire::Fixed(1),
                        key: &Wire::Fixed(4),
                        full: &Wire::Seq(&[Wire::Fixed(4), Wire::Fixed(1)]),
                    },
                },
                Child {
                    name: "buyFamilyCounts",
                    wire: Wire::List {
                        value: &Wire::Fixed(1),
                        key: &Wire::Fixed(4),
                        full: &Wire::Seq(&[Wire::Fixed(4), Wire::Fixed(1)]),
                    },
                },
                Child {
                    name: "buyGroupCounts",
                    wire: Wire::List {
                        value: &Wire::Fixed(1),
                        key: &Wire::Fixed(4),
                        full: &Wire::Seq(&[Wire::Fixed(4), Wire::Fixed(1)]),
                    },
                },
                Child {
                    name: "buyItemCounts",
                    wire: Wire::List {
                        value: &Wire::Fixed(1),
                        key: &Wire::Fixed(4),
                        full: &Wire::Seq(&[Wire::Fixed(4), Wire::Fixed(1)]),
                    },
                },
                Child {
                    name: "sellCategoryCounts",
                    wire: Wire::List {
                        value: &Wire::Fixed(1),
                        key: &Wire::Fixed(4),
                        full: &Wire::Seq(&[Wire::Fixed(4), Wire::Fixed(1)]),
                    },
                },
                Child {
                    name: "sellFamilyCounts",
                    wire: Wire::List {
                        value: &Wire::Fixed(1),
                        key: &Wire::Fixed(4),
                        full: &Wire::Seq(&[Wire::Fixed(4), Wire::Fixed(1)]),
                    },
                },
                Child {
                    name: "sellGroupCounts",
                    wire: Wire::List {
                        value: &Wire::Fixed(1),
                        key: &Wire::Fixed(4),
                        full: &Wire::Seq(&[Wire::Fixed(4), Wire::Fixed(1)]),
                    },
                },
                Child {
                    name: "sellItemCounts",
                    wire: Wire::List {
                        value: &Wire::Fixed(1),
                        key: &Wire::Fixed(4),
                        full: &Wire::Seq(&[Wire::Fixed(4), Wire::Fixed(1)]),
                    },
                },
            ],
            &[],
        ],
    },
    ClassLayout {
        class: 3829,
        name: "MB::ObjectiveInteractorComponentReplicatedState",
        slots: &[
            &[
                Child {
                    name: "objectiveProviderId",
                    wire: Wire::Fixed(16),
                },
                Child {
                    name: "expirationTime",
                    wire: Wire::Fixed(8),
                },
                Child {
                    name: "missionParams",
                    wire: Wire::List {
                        value: &Wire::Seq(&[
                            Wire::Fixed(8),
                            Wire::Fixed(4),
                            Wire::Seq(&[
                                Wire::Fixed(16),
                                Wire::Fixed(8),
                                Wire::Fixed(2),
                                Wire::Fixed(1),
                                Wire::Fixed(1),
                                Wire::Fixed(1),
                                Wire::Seq(&[
                                    Wire::Fixed(1),
                                    Wire::Fixed(8),
                                    Wire::Fixed(1),
                                    Wire::Array(&Wire::Fixed(4)),
                                ]),
                            ]),
                        ]),
                        key: &Wire::Wide,
                        full: &Wire::Seq(&[
                            Wire::Fixed(8),
                            Wire::Fixed(4),
                            Wire::Seq(&[
                                Wire::Fixed(16),
                                Wire::Fixed(8),
                                Wire::Fixed(2),
                                Wire::Fixed(1),
                                Wire::Fixed(1),
                                Wire::Fixed(1),
                                Wire::Seq(&[
                                    Wire::Fixed(1),
                                    Wire::Fixed(8),
                                    Wire::Fixed(1),
                                    Wire::Array(&Wire::Fixed(4)),
                                ]),
                            ]),
                        ]),
                    },
                },
                Child {
                    name: "m_communityGoals",
                    wire: Wire::Array(&Wire::Seq(&[
                        Wire::Fixed(2),
                        Wire::Fixed(4),
                        Wire::Array(&Wire::Fixed(4)),
                        Wire::Array(&Wire::Fixed(4)),
                    ])),
                },
            ],
            &[],
        ],
    },
    ClassLayout {
        class: 3857,
        name: "Javelin::ObjectivesComponentReplicatedState",
        slots: &[
            &[Child {
                name: "gracePeriodEndTime",
                wire: Wire::Fixed(8),
            }],
            &[
                Child {
                    name: "taskStartTimes",
                    wire: Wire::List {
                        value: &Wire::Fixed(8),
                        key: &Wire::Seq(&[Wire::Fixed(8), Wire::Fixed(1)]),
                        full: &Wire::Seq(&[
                            Wire::Seq(&[Wire::Fixed(8), Wire::Fixed(1)]),
                            Wire::Fixed(8),
                        ]),
                    },
                },
                Child {
                    name: "trackedObjectives",
                    wire: Wire::List {
                        value: &Wire::Fixed(8),
                        key: &Wire::Wide,
                        full: &Wire::Fixed(8),
                    },
                },
                Child {
                    name: "completedObjectives",
                    wire: Wire::List {
                        value: &Wire::Fixed(8),
                        key: &Wire::Wide,
                        full: &Wire::Fixed(8),
                    },
                },
                Child {
                    name: "activeObjectives",
                    wire: Wire::List {
                        value: &Wire::Seq(&[
                            Wire::Fixed(4),
                            Wire::Seq(&[
                                Wire::Fixed(8),
                                Wire::Fixed(4),
                                Wire::Seq(&[
                                    Wire::Fixed(16),
                                    Wire::Fixed(8),
                                    Wire::Fixed(2),
                                    Wire::Fixed(1),
                                    Wire::Fixed(1),
                                    Wire::Fixed(1),
                                    Wire::Seq(&[
                                        Wire::Fixed(1),
                                        Wire::Fixed(8),
                                        Wire::Fixed(1),
                                        Wire::Array(&Wire::Fixed(4)),
                                    ]),
                                ]),
                            ]),
                        ]),
                        key: &Wire::Wide,
                        full: &Wire::Seq(&[
                            Wire::Fixed(4),
                            Wire::Seq(&[
                                Wire::Fixed(8),
                                Wire::Fixed(4),
                                Wire::Seq(&[
                                    Wire::Fixed(16),
                                    Wire::Fixed(8),
                                    Wire::Fixed(2),
                                    Wire::Fixed(1),
                                    Wire::Fixed(1),
                                    Wire::Fixed(1),
                                    Wire::Seq(&[
                                        Wire::Fixed(1),
                                        Wire::Fixed(8),
                                        Wire::Fixed(1),
                                        Wire::Array(&Wire::Fixed(4)),
                                    ]),
                                ]),
                            ]),
                        ]),
                    },
                },
                Child {
                    name: "taskStates",
                    wire: Wire::List {
                        value: &Wire::Seq(&[
                            Wire::Fixed(8),
                            Wire::Fixed(1),
                            Wire::Fixed(4),
                            Wire::Fixed(4),
                            Wire::Fixed(1),
                        ]),
                        key: &Wire::Wide,
                        full: &Wire::Seq(&[
                            Wire::Fixed(8),
                            Wire::Fixed(1),
                            Wire::Fixed(4),
                            Wire::Fixed(4),
                            Wire::Fixed(1),
                        ]),
                    },
                },
                Child {
                    name: "objectivePoiEntityIds",
                    wire: Wire::List {
                        value: &Wire::Fixed(8),
                        key: &Wire::Wide,
                        full: &Wire::Fixed(8),
                    },
                },
                Child {
                    name: "dynamicPoiIndices",
                    wire: Wire::List {
                        value: &Wire::Fixed(2),
                        key: &Wire::Wide,
                        full: &Wire::Fixed(2),
                    },
                },
            ],
            &[],
        ],
    },
    ClassLayout {
        class: 3935,
        name: "MB::PlayerComponentReplicatedState",
        slots: &[
            &[],
            &[
                Child {
                    name: "loginMatchId",
                    wire: Wire::Text,
                },
                Child {
                    name: "srcWorldId",
                    wire: Wire::FlagId,
                },
                Child {
                    name: "accountIsLocked",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "accountInProbation",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "ageGroup",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "territoryOwnerGuildId",
                    wire: Wire::Fixed(16),
                },
                Child {
                    name: "sessionStartWallClockTimePoint",
                    wire: Wire::Fixed(8),
                },
                Child {
                    name: "sessionStartTimePoint",
                    wire: Wire::Fixed(8),
                },
                Child {
                    name: "debugAccountProbationOverride",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "freePlayerCountdown",
                    wire: Wire::Seq(&[Wire::Fixed(8), Wire::Fixed(1)]),
                },
                Child {
                    name: "enteringStoreIsBlocked",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "isFreshStartWorld",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "onDeathRespawnCooldown",
                    wire: Wire::Fixed(4),
                },
                Child {
                    name: "mostRecentPVPActiveSwitchTimePoint",
                    wire: Wire::Fixed(8),
                },
                Child {
                    name: "shouldNotifyPlayer",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "isPVPActiveCharacter",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "isChangingMount",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "isTransmogStationScreenOpen",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "isTransmogScreenOpen",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "isInMountAttachmentMode",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "isArmorDyeingOpen",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "playerBackstory",
                    wire: Wire::Fixed(4),
                },
            ],
            &[
                Child {
                    name: "characterId",
                    wire: Wire::FlagId,
                },
                Child {
                    name: "characterName",
                    wire: Wire::Text,
                },
                Child {
                    name: "homeWorldId",
                    wire: Wire::FlagId,
                },
                Child {
                    name: "playerConnected",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "lookingThroughLoadout",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "playerType",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "isInStore",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "platformAccountId",
                    wire: Wire::Fixed(8),
                },
                Child {
                    name: "platformType",
                    wire: Wire::Fixed(1),
                },
            ],
            &[],
        ],
    },
    ClassLayout {
        class: 4005,
        name: "MB::SpawnerComponentReplicatedState",
        slots: &[
            &[
                Child {
                    name: "numActiveSpawns",
                    wire: Wire::Fixed(4),
                },
                Child {
                    name: "spawnTag",
                    wire: Wire::Fixed(4),
                },
                Child {
                    name: "sourceSpawnerGDERef",
                    wire: Wire::Fixed(24),
                },
            ],
            &[],
        ],
    },
    ClassLayout {
        class: 4103,
        name: "MB::SiegeWarfareDataReplicatedState",
        slots: &[
            &[
                Child {
                    name: "capturePointStatesColdData",
                    wire: Wire::List {
                        value: &Wire::Seq(&[
                            Wire::Seq(&[Wire::Fixed(4), Wire::Fixed(4), Wire::Fixed(4)]),
                            Wire::Fixed(4),
                            Wire::Fixed(1),
                            Wire::Seq(&[Wire::Fixed(8), Wire::Fixed(1)]),
                        ]),
                        key: &Wire::Fixed(8),
                        full: &Wire::Seq(&[
                            Wire::Fixed(8),
                            Wire::Seq(&[
                                Wire::Seq(&[Wire::Fixed(4), Wire::Fixed(4), Wire::Fixed(4)]),
                                Wire::Fixed(4),
                                Wire::Fixed(1),
                                Wire::Seq(&[Wire::Fixed(8), Wire::Fixed(1)]),
                            ]),
                        ]),
                    },
                },
                Child {
                    name: "capturePointStatesHotData",
                    wire: Wire::List {
                        value: &Wire::Fixed(1),
                        key: &Wire::Fixed(8),
                        full: &Wire::Seq(&[Wire::Fixed(8), Wire::Fixed(1)]),
                    },
                },
                Child {
                    name: "fortMajorStructureStates",
                    wire: Wire::List {
                        value: &Wire::Seq(&[
                            Wire::Seq(&[Wire::Fixed(4), Wire::Fixed(4), Wire::Fixed(4)]),
                            Wire::Fixed(4),
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                        ]),
                        key: &Wire::Fixed(8),
                        full: &Wire::Seq(&[
                            Wire::Fixed(8),
                            Wire::Seq(&[
                                Wire::Seq(&[Wire::Fixed(4), Wire::Fixed(4), Wire::Fixed(4)]),
                                Wire::Fixed(4),
                                Wire::Fixed(1),
                                Wire::Fixed(1),
                            ]),
                        ]),
                    },
                },
                Child {
                    name: "siegeWeaponStates",
                    wire: Wire::List {
                        value: &Wire::Seq(&[
                            Wire::Fixed(12),
                            Wire::Fixed(4),
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                        ]),
                        key: &Wire::Fixed(8),
                        full: &Wire::Seq(&[
                            Wire::Fixed(8),
                            Wire::Seq(&[
                                Wire::Fixed(12),
                                Wire::Fixed(4),
                                Wire::Fixed(1),
                                Wire::Fixed(1),
                            ]),
                        ]),
                    },
                },
                Child {
                    name: "numAttackerBuildablesBoundByTeamLimit",
                    wire: Wire::List {
                        value: &Wire::Fixed(1),
                        key: &Wire::Fixed(4),
                        full: &Wire::Seq(&[Wire::Fixed(4), Wire::Fixed(1)]),
                    },
                },
                Child {
                    name: "numDefenderBuildablesBoundByTeamLimit",
                    wire: Wire::List {
                        value: &Wire::Fixed(1),
                        key: &Wire::Fixed(4),
                        full: &Wire::Seq(&[Wire::Fixed(4), Wire::Fixed(1)]),
                    },
                },
            ],
            &[],
        ],
    },
    ClassLayout {
        class: 4176,
        name: "MB::SocialReplicatedState",
        slots: &[
            &[
                Child {
                    name: "playerTitleId",
                    wire: Wire::Fixed(4),
                },
                Child {
                    name: "pronounType",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "chattingStateMessageType",
                    wire: Wire::Fixed(4),
                },
            ],
            &[
                Child {
                    name: "warData",
                    wire: Wire::List {
                        value: &Wire::Seq(&[
                            Wire::Fixed(16),
                            Wire::Fixed(16),
                            Wire::Fixed(8),
                            Wire::Array(&Wire::Fixed(16)),
                            Wire::Fixed(16),
                            Wire::Fixed(8),
                            Wire::Seq(&[
                                Wire::Fixed(8),
                                Wire::Array(&Wire::Fixed(16)),
                                Wire::Fixed(16),
                                Wire::Fixed(8),
                                Wire::Fixed(1),
                                Wire::Fixed(1),
                                Wire::Fixed(2),
                                Wire::Fixed(4),
                                Wire::Fixed(16),
                                Wire::Fixed(8),
                                Wire::Seq(&[
                                    Wire::Fixed(16),
                                    Wire::Fixed(1),
                                    Wire::Fixed(4),
                                    Wire::Fixed(1),
                                    Wire::Fixed(1),
                                    Wire::Fixed(4),
                                    Wire::Fixed(8),
                                    Wire::Seq(&[Wire::Fixed(4), Wire::Fixed(16), Wire::Fixed(16)]),
                                ]),
                            ]),
                        ]),
                        key: &Wire::Fixed(16),
                        full: &Wire::Seq(&[
                            Wire::Fixed(16),
                            Wire::Seq(&[
                                Wire::Fixed(16),
                                Wire::Fixed(16),
                                Wire::Fixed(8),
                                Wire::Array(&Wire::Fixed(16)),
                                Wire::Fixed(16),
                                Wire::Fixed(8),
                                Wire::Seq(&[
                                    Wire::Fixed(8),
                                    Wire::Array(&Wire::Fixed(16)),
                                    Wire::Fixed(16),
                                    Wire::Fixed(8),
                                    Wire::Fixed(1),
                                    Wire::Fixed(1),
                                    Wire::Fixed(2),
                                    Wire::Fixed(4),
                                    Wire::Fixed(16),
                                    Wire::Fixed(8),
                                    Wire::Seq(&[
                                        Wire::Fixed(16),
                                        Wire::Fixed(1),
                                        Wire::Fixed(4),
                                        Wire::Fixed(1),
                                        Wire::Fixed(1),
                                        Wire::Fixed(4),
                                        Wire::Fixed(8),
                                        Wire::Seq(&[
                                            Wire::Fixed(4),
                                            Wire::Fixed(16),
                                            Wire::Fixed(16),
                                        ]),
                                    ]),
                                ]),
                            ]),
                        ]),
                    },
                },
                Child {
                    name: "dailyWarAsAttackerCount",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "dailyWarAsDefenderCount",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "lastDailyResetTime",
                    wire: Wire::Fixed(8),
                },
                Child {
                    name: "friends",
                    wire: Wire::List {
                        value: &Wire::Text,
                        key: &Wire::Wide,
                        full: &Wire::Text,
                    },
                },
                Child {
                    name: "friendInvites",
                    wire: Wire::List {
                        value: &Wire::Text,
                        key: &Wire::Wide,
                        full: &Wire::Text,
                    },
                },
                Child {
                    name: "socialBlocks",
                    wire: Wire::List {
                        value: &Wire::Text,
                        key: &Wire::Wide,
                        full: &Wire::Text,
                    },
                },
                Child {
                    name: "mostRecentJoinCharacterCall",
                    wire: Wire::Fixed(8),
                },
            ],
            &[],
        ],
    },
    ClassLayout {
        class: 4236,
        name: "MB::StatusEffectsComponentReplicatedState",
        slots: &[
            &[],
            &[
                Child {
                    name: "m_localEffectsMap",
                    wire: Wire::List {
                        value: &Wire::Seq(&[
                            Wire::Fixed(8),
                            Wire::Fixed(2),
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                        ]),
                        key: &Wire::Fixed(2),
                        full: &Wire::Seq(&[
                            Wire::Fixed(2),
                            Wire::Seq(&[
                                Wire::Fixed(8),
                                Wire::Fixed(2),
                                Wire::Fixed(1),
                                Wire::Fixed(1),
                            ]),
                        ]),
                    },
                },
                Child {
                    name: "m_lightweightLocalEffectsMap",
                    wire: Wire::List {
                        value: &Wire::Fixed(8),
                        key: &Wire::Fixed(2),
                        full: &Wire::Seq(&[Wire::Fixed(2), Wire::Fixed(8)]),
                    },
                },
                Child {
                    name: "m_activeTrayIcons",
                    wire: Wire::List {
                        value: &Wire::Seq(&[
                            Wire::Fixed(4),
                            Wire::Fixed(8),
                            Wire::Fixed(1),
                            Wire::Fixed(4),
                        ]),
                        key: &Wire::Wide,
                        full: &Wire::Seq(&[
                            Wire::Fixed(4),
                            Wire::Fixed(8),
                            Wire::Fixed(1),
                            Wire::Fixed(4),
                        ]),
                    },
                },
            ],
            &[
                Child {
                    name: "m_territoryStatusEffects",
                    wire: Wire::List {
                        value: &Wire::Seq(&[Wire::Fixed(4), Wire::Fixed(8)]),
                        key: &Wire::Wide,
                        full: &Wire::Seq(&[Wire::Fixed(4), Wire::Fixed(8)]),
                    },
                },
                Child {
                    name: "m_dynamicScalingData",
                    wire: Wire::List {
                        value: &Wire::Seq(&[
                            Wire::Fixed(4),
                            Wire::Fixed(4),
                            Wire::Fixed(4),
                            Wire::Seq(&[Wire::Fixed(8), Wire::Fixed(8)]),
                        ]),
                        key: &Wire::Wide,
                        full: &Wire::Seq(&[
                            Wire::Fixed(4),
                            Wire::Fixed(4),
                            Wire::Fixed(4),
                            Wire::Seq(&[Wire::Fixed(8), Wire::Fixed(8)]),
                        ]),
                    },
                },
                Child {
                    name: "m_localReplicatedUpdateCounts",
                    wire: Wire::List {
                        value: &Wire::Fixed(2),
                        key: &Wire::Fixed(4),
                        full: &Wire::Seq(&[Wire::Fixed(4), Wire::Fixed(2)]),
                    },
                },
                Child {
                    name: "m_remoteReplicatedUpdateCounts",
                    wire: Wire::List {
                        value: &Wire::Fixed(2),
                        key: &Wire::Fixed(4),
                        full: &Wire::Seq(&[Wire::Fixed(4), Wire::Fixed(2)]),
                    },
                },
            ],
            &[
                Child {
                    name: "m_effectsMap",
                    wire: Wire::List {
                        value: &Wire::Seq(&[
                            Wire::Fixed(8),
                            Wire::Fixed(2),
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                        ]),
                        key: &Wire::Fixed(2),
                        full: &Wire::Seq(&[
                            Wire::Fixed(2),
                            Wire::Seq(&[
                                Wire::Fixed(8),
                                Wire::Fixed(2),
                                Wire::Fixed(1),
                                Wire::Fixed(1),
                            ]),
                        ]),
                    },
                },
                Child {
                    name: "m_remoteEffectsMap",
                    wire: Wire::List {
                        value: &Wire::Fixed(1),
                        key: &Wire::Fixed(2),
                        full: &Wire::Seq(&[Wire::Fixed(2), Wire::Fixed(1)]),
                    },
                },
            ],
            &[],
        ],
    },
    ClassLayout {
        class: 4251,
        name: "Javelin::P2PTradeReplicatedState",
        slots: &[
            &[
                Child {
                    name: "ownOfferedItems",
                    wire: Wire::List {
                        value: &Wire::Seq(&[
                            Wire::Fixed(8),
                            Wire::Prefixed,
                            Wire::Prefixed,
                            Wire::Prefixed,
                            Wire::Prefixed,
                            Wire::Prefixed,
                            Wire::Prefixed,
                            Wire::Prefixed,
                            Wire::Prefixed,
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                            Wire::Fixed(16),
                            Wire::Fixed(1),
                            Wire::Fixed(4),
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                        ]),
                        key: &Wire::Wide,
                        full: &Wire::Seq(&[
                            Wire::Fixed(8),
                            Wire::Prefixed,
                            Wire::Prefixed,
                            Wire::Prefixed,
                            Wire::Prefixed,
                            Wire::Prefixed,
                            Wire::Prefixed,
                            Wire::Prefixed,
                            Wire::Prefixed,
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                            Wire::Fixed(16),
                            Wire::Fixed(1),
                            Wire::Fixed(4),
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                        ]),
                    },
                },
                Child {
                    name: "ownOfferedCoin",
                    wire: Wire::Fixed(8),
                },
                Child {
                    name: "ownOfferIsLockedIn",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "otherOfferedItems",
                    wire: Wire::List {
                        value: &Wire::Seq(&[
                            Wire::Fixed(8),
                            Wire::Prefixed,
                            Wire::Prefixed,
                            Wire::Prefixed,
                            Wire::Prefixed,
                            Wire::Prefixed,
                            Wire::Prefixed,
                            Wire::Prefixed,
                            Wire::Prefixed,
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                            Wire::Fixed(16),
                            Wire::Fixed(1),
                            Wire::Fixed(4),
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                        ]),
                        key: &Wire::Wide,
                        full: &Wire::Seq(&[
                            Wire::Fixed(8),
                            Wire::Prefixed,
                            Wire::Prefixed,
                            Wire::Prefixed,
                            Wire::Prefixed,
                            Wire::Prefixed,
                            Wire::Prefixed,
                            Wire::Prefixed,
                            Wire::Prefixed,
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                            Wire::Fixed(16),
                            Wire::Fixed(1),
                            Wire::Fixed(4),
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                        ]),
                    },
                },
                Child {
                    name: "otherOfferedCoin",
                    wire: Wire::Fixed(8),
                },
                Child {
                    name: "otherOfferIsLockedIn",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "canReceiveOtherItems",
                    wire: Wire::Fixed(1),
                },
            ],
            &[],
        ],
    },
    ClassLayout {
        class: 4276,
        name: "MB::TurretReplicatedState",
        slots: &[
            &[
                Child {
                    name: "yaw",
                    wire: Wire::Fixed(2),
                },
                Child {
                    name: "pitch",
                    wire: Wire::Fixed(2),
                },
                Child {
                    name: "heat",
                    wire: Wire::Fixed(2),
                },
                Child {
                    name: "initialOrientation",
                    wire: Wire::PackedTurn,
                },
                Child {
                    name: "interactorGDERef",
                    wire: Wire::Fixed(24),
                },
                Child {
                    name: "isOverheated",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "isInteractionBlocked",
                    wire: Wire::Fixed(1),
                },
            ],
            &[],
        ],
    },
    ClassLayout {
        class: 4297,
        name: "MB::StaminaComponentReplicatedState",
        slots: &[
            &[
                Child {
                    name: "cur",
                    wire: Wire::Fixed(4),
                },
                Child {
                    name: "max",
                    wire: Wire::Fixed(4),
                },
                Child {
                    name: "winded",
                    wire: Wire::Fixed(4),
                },
                Child {
                    name: "regen",
                    wire: Wire::Fixed(4),
                },
                Child {
                    name: "multMax",
                    wire: Wire::Fixed(4),
                },
                Child {
                    name: "multRegen",
                    wire: Wire::Fixed(4),
                },
            ],
            &[],
        ],
    },
    ClassLayout {
        class: 4320,
        name: "MB::WaterLevelComponentReplicatedState",
        slots: &[
            &[Child {
                name: "m_isBeingGatheredFrom",
                wire: Wire::Fixed(1),
            }],
            &[],
        ],
    },
    ClassLayout {
        class: 4321,
        name: "MB::WaypointsComponentReplicatedState",
        slots: &[
            &[Child {
                name: "replicatedWaypointPosition",
                wire: Wire::Fixed(12),
            }],
            &[],
        ],
    },
    ClassLayout {
        class: 4878,
        name: "Javelin::MusicalPerformancePlayerComponentReplicatedState",
        slots: &[
            &[Child {
                name: "performanceId",
                wire: Wire::Fixed(8),
            }],
            &[
                Child {
                    name: "lockoutDuration",
                    wire: Wire::Fixed(2),
                },
                Child {
                    name: "performanceState",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "observingPerformanceId",
                    wire: Wire::Fixed(8),
                },
            ],
            &[],
        ],
    },
    ClassLayout {
        class: 4913,
        name: "MB::RewardTrackComponentReplicatedState",
        slots: &[
            &[],
            &[Child {
                name: "m_pvpXpRank",
                wire: Wire::Fixed(2),
            }],
            &[
                Child {
                    name: "m_rolledRewards",
                    wire: Wire::List {
                        value: &Wire::Seq(&[
                            Wire::Fixed(4),
                            Wire::Fixed(4),
                            Wire::Seq(&[
                                Wire::Fixed(8),
                                Wire::Prefixed,
                                Wire::Prefixed,
                                Wire::Prefixed,
                                Wire::Prefixed,
                                Wire::Prefixed,
                                Wire::Prefixed,
                                Wire::Prefixed,
                                Wire::Prefixed,
                                Wire::Fixed(1),
                                Wire::Fixed(1),
                                Wire::Fixed(1),
                                Wire::Fixed(1),
                                Wire::Fixed(16),
                                Wire::Fixed(1),
                                Wire::Fixed(4),
                                Wire::Fixed(1),
                                Wire::Fixed(1),
                                Wire::Fixed(1),
                                Wire::Fixed(1),
                            ]),
                            Wire::Fixed(1),
                        ]),
                        key: &Wire::Wide,
                        full: &Wire::Seq(&[
                            Wire::Fixed(4),
                            Wire::Fixed(4),
                            Wire::Seq(&[
                                Wire::Fixed(8),
                                Wire::Prefixed,
                                Wire::Prefixed,
                                Wire::Prefixed,
                                Wire::Prefixed,
                                Wire::Prefixed,
                                Wire::Prefixed,
                                Wire::Prefixed,
                                Wire::Prefixed,
                                Wire::Fixed(1),
                                Wire::Fixed(1),
                                Wire::Fixed(1),
                                Wire::Fixed(1),
                                Wire::Fixed(16),
                                Wire::Fixed(1),
                                Wire::Fixed(4),
                                Wire::Fixed(1),
                                Wire::Fixed(1),
                                Wire::Fixed(1),
                                Wire::Fixed(1),
                            ]),
                            Wire::Fixed(1),
                        ]),
                    },
                },
                Child {
                    name: "m_selectedRewards",
                    wire: Wire::List {
                        value: &Wire::Fixed(1),
                        key: &Wire::Wide,
                        full: &Wire::Fixed(1),
                    },
                },
                Child {
                    name: "m_debugTrackExcludedTags",
                    wire: Wire::List {
                        value: &Wire::Fixed(4),
                        key: &Wire::Wide,
                        full: &Wire::Fixed(4),
                    },
                },
            ],
            &[],
        ],
    },
    ClassLayout {
        class: 5027,
        name: "MB::TippingPoolComponentReplicatedState",
        slots: &[
            &[
                Child {
                    name: "replicatedPerformerEntityIds",
                    wire: Wire::List {
                        value: &Wire::Fixed(8),
                        key: &Wire::Wide,
                        full: &Wire::Fixed(8),
                    },
                },
                Child {
                    name: "replicatedTipDonationCost",
                    wire: Wire::Fixed(8),
                },
                Child {
                    name: "replicatedTippingPoolCloseTimePoint",
                    wire: Wire::Fixed(8),
                },
                Child {
                    name: "replicatedTippingPoolOpen",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "replicatedTriggerSphereRadius",
                    wire: Wire::Fixed(4),
                },
            ],
            &[],
        ],
    },
    ClassLayout {
        class: 5257,
        name: "MB::ChargeComponentReplicatedState",
        slots: &[
            &[Child {
                name: "chrgPcnt",
                wire: Wire::Fixed(1),
            }],
            &[],
        ],
    },
    ClassLayout {
        class: 5399,
        name: "MB::MarkerComponentReplicatedState",
        slots: &[
            &[Child {
                name: "enabled",
                wire: Wire::Fixed(1),
            }],
            &[],
        ],
    },
    ClassLayout {
        class: 5437,
        name: "MB::ItemManagementComponentReplicatedState",
        slots: &[
            &[],
            &[Child {
                name: "ownedItems",
                wire: Wire::List {
                    value: &Wire::Seq(&[
                        Wire::Fixed(8),
                        Wire::Prefixed,
                        Wire::Prefixed,
                        Wire::Prefixed,
                        Wire::Prefixed,
                        Wire::Prefixed,
                        Wire::Prefixed,
                        Wire::Prefixed,
                        Wire::Prefixed,
                        Wire::Fixed(1),
                        Wire::Fixed(1),
                        Wire::Fixed(1),
                        Wire::Fixed(1),
                        Wire::Fixed(16),
                        Wire::Fixed(1),
                        Wire::Fixed(4),
                        Wire::Fixed(1),
                        Wire::Fixed(1),
                        Wire::Fixed(1),
                        Wire::Fixed(1),
                    ]),
                    key: &Wire::Fixed(2),
                    full: &Wire::Seq(&[
                        Wire::Fixed(2),
                        Wire::Seq(&[
                            Wire::Fixed(8),
                            Wire::Prefixed,
                            Wire::Prefixed,
                            Wire::Prefixed,
                            Wire::Prefixed,
                            Wire::Prefixed,
                            Wire::Prefixed,
                            Wire::Prefixed,
                            Wire::Prefixed,
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                            Wire::Fixed(16),
                            Wire::Fixed(1),
                            Wire::Fixed(4),
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                            Wire::Fixed(1),
                        ]),
                    ]),
                },
            }],
            &[],
        ],
    },
    ClassLayout {
        class: 5485,
        name: "Javelin::SeasonsRewardsReplicatedState",
        slots: &[
            &[
                Child {
                    name: "cardTemplate",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "claimedTasks",
                    wire: Wire::List {
                        value: &Wire::Fixed(2),
                        key: &Wire::Wide,
                        full: &Wire::Fixed(2),
                    },
                },
                Child {
                    name: "stampedSquares",
                    wire: Wire::List {
                        value: &Wire::Fixed(2),
                        key: &Wire::Wide,
                        full: &Wire::Fixed(2),
                    },
                },
                Child {
                    name: "wildStampedSquares",
                    wire: Wire::List {
                        value: &Wire::Fixed(2),
                        key: &Wire::Wide,
                        full: &Wire::Fixed(2),
                    },
                },
                Child {
                    name: "rewardClaimed",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "rerollCount",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "activitiesTasks",
                    wire: Wire::Array(&Wire::Fixed(4)),
                },
                Child {
                    name: "cardCount",
                    wire: Wire::Fixed(2),
                },
                Child {
                    name: "wildStampCount",
                    wire: Wire::Fixed(2),
                },
                Child {
                    name: "wildStampAwardBound",
                    wire: Wire::Fixed(8),
                },
                Child {
                    name: "wildStampAwardsThisSession",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "wildStampAwardRemaining",
                    wire: Wire::Fixed(2),
                },
                Child {
                    name: "IsInitialized",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "SeasonIds",
                    wire: Wire::List {
                        value: &Wire::Fixed(4),
                        key: &Wire::Wide,
                        full: &Wire::Fixed(4),
                    },
                },
                Child {
                    name: "SeasonBitmaskCount",
                    wire: Wire::List {
                        value: &Wire::Fixed(1),
                        key: &Wire::Wide,
                        full: &Wire::Fixed(1),
                    },
                },
                Child {
                    name: "SeasonXpBySeason",
                    wire: Wire::List {
                        value: &Wire::Fixed(8),
                        key: &Wire::Wide,
                        full: &Wire::Fixed(8),
                    },
                },
                Child {
                    name: "RedeemBitmask",
                    wire: Wire::List {
                        value: &Wire::Fixed(8),
                        key: &Wire::Wide,
                        full: &Wire::Fixed(8),
                    },
                },
                Child {
                    name: "EscrowBitmask",
                    wire: Wire::List {
                        value: &Wire::Fixed(8),
                        key: &Wire::Wide,
                        full: &Wire::Fixed(8),
                    },
                },
                Child {
                    name: "ForeignEscrowBitmask",
                    wire: Wire::List {
                        value: &Wire::Fixed(8),
                        key: &Wire::Wide,
                        full: &Wire::Fixed(8),
                    },
                },
                Child {
                    name: "firstCharacterConnectTime",
                    wire: Wire::Fixed(8),
                },
            ],
            &[],
        ],
    },
    ClassLayout {
        class: 5606,
        name: "Javelin::SeasonsRewardsTrackedStatReplicatedState",
        slots: &[
            &[
                Child {
                    name: "initialized",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "groupList",
                    wire: Wire::List {
                        value: &Wire::Fixed(4),
                        key: &Wire::Wide,
                        full: &Wire::Fixed(4),
                    },
                },
                Child {
                    name: "groupCountList",
                    wire: Wire::List {
                        value: &Wire::Fixed(2),
                        key: &Wire::Wide,
                        full: &Wire::Fixed(2),
                    },
                },
                Child {
                    name: "groupStatIndex",
                    wire: Wire::List {
                        value: &Wire::Fixed(2),
                        key: &Wire::Wide,
                        full: &Wire::Fixed(2),
                    },
                },
                Child {
                    name: "groupStatValue",
                    wire: Wire::List {
                        value: &Wire::Fixed(4),
                        key: &Wire::Wide,
                        full: &Wire::Fixed(4),
                    },
                },
            ],
            &[],
        ],
    },
    ClassLayout {
        class: 5620,
        name: "MB::MountComponentReplicatedState",
        slots: &[
            &[],
            &[
                Child {
                    name: "m_isMounted",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "m_summonCooldownEndTime",
                    wire: Wire::Fixed(8),
                },
                Child {
                    name: "m_isServerForcingWalk",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "m_isInServerExclusionVolume",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "m_summonAuthorization",
                    wire: Wire::Seq(&[Wire::Fixed(1), Wire::Fixed(8)]),
                },
                Child {
                    name: "m_persistentMountData",
                    wire: Wire::List {
                        value: &Wire::Seq(&[
                            Wire::Seq(&[
                                Wire::Fixed(1),
                                Wire::Fixed(1),
                                Wire::Fixed(1),
                                Wire::Fixed(1),
                            ]),
                            Wire::Text,
                        ]),
                        key: &Wire::Fixed(4),
                        full: &Wire::Seq(&[
                            Wire::Fixed(4),
                            Wire::Seq(&[
                                Wire::Fixed(1),
                                Wire::Fixed(1),
                                Wire::Fixed(1),
                                Wire::Fixed(1),
                            ]),
                            Wire::Text,
                        ]),
                    },
                },
                Child {
                    name: "staminaCur",
                    wire: Wire::Fixed(4),
                },
                Child {
                    name: "staminaMax",
                    wire: Wire::Fixed(4),
                },
                Child {
                    name: "staminaRegenDelay",
                    wire: Wire::Fixed(4),
                },
                Child {
                    name: "staminaRegenRate",
                    wire: Wire::Fixed(4),
                },
                Child {
                    name: "staminaDrainRate",
                    wire: Wire::Fixed(4),
                },
                Child {
                    name: "multMaxStamina",
                    wire: Wire::Fixed(4),
                },
                Child {
                    name: "multStaminaRegenRate",
                    wire: Wire::Fixed(4),
                },
            ],
            &[Child {
                name: "m_mountId",
                wire: Wire::Fixed(4),
            }],
            &[
                Child {
                    name: "mountRemoteFlags",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "remoteDyeData",
                    wire: Wire::Seq(&[
                        Wire::Fixed(1),
                        Wire::Fixed(1),
                        Wire::Fixed(1),
                        Wire::Fixed(1),
                    ]),
                },
            ],
            &[],
        ],
    },
    ClassLayout {
        class: 5691,
        name: "MB::TransmogComponentReplicatedState",
        slots: &[
            &[
                Child {
                    name: "capturedArmorAppearances",
                    wire: Wire::List {
                        value: &Wire::Fixed(8),
                        key: &Wire::Wide,
                        full: &Wire::Fixed(8),
                    },
                },
                Child {
                    name: "capturedWeaponAppearances",
                    wire: Wire::List {
                        value: &Wire::Fixed(8),
                        key: &Wire::Wide,
                        full: &Wire::Fixed(8),
                    },
                },
                Child {
                    name: "ownedArmorAppearances",
                    wire: Wire::List {
                        value: &Wire::Fixed(8),
                        key: &Wire::Wide,
                        full: &Wire::Fixed(8),
                    },
                },
                Child {
                    name: "ownedWeaponAppearances",
                    wire: Wire::List {
                        value: &Wire::Fixed(8),
                        key: &Wire::Wide,
                        full: &Wire::Fixed(8),
                    },
                },
                Child {
                    name: "inventoryServicesReady",
                    wire: Wire::Fixed(1),
                },
            ],
            &[],
        ],
    },
    ClassLayout {
        class: 5915,
        name: "MB::ClientPathingComponentReplicatedState",
        slots: &[
            &[Child {
                name: "corridorPaths",
                wire: Wire::Seq(&[
                    Wire::Array(&Wire::Seq(&[
                        Wire::Fixed(1),
                        Wire::Seq(&[Wire::Fixed(4), Wire::Fixed(4), Wire::Fixed(4)]),
                        Wire::Fixed(4),
                        Wire::Array(&Wire::Seq(&[
                            Wire::Fixed(2),
                            Wire::Fixed(2),
                            Wire::Fixed(2),
                        ])),
                        Wire::Array(&Wire::Fixed(2)),
                    ])),
                    Wire::Fixed(4),
                ]),
            }],
            &[],
        ],
    },
    ClassLayout {
        class: 5980,
        name: "MB::TransformLinkComponentReplicatedState",
        slots: &[
            &[Child {
                name: "m_childGDEVector",
                wire: Wire::List {
                    value: &Wire::Seq(&[Wire::Fixed(16), Wire::Fixed(8)]),
                    key: &Wire::Wide,
                    full: &Wire::Seq(&[Wire::Fixed(16), Wire::Fixed(8)]),
                },
            }],
            &[],
        ],
    },
    ClassLayout {
        class: 6044,
        name: "MB::PlayerQuickCourseComponentReplicatedState",
        slots: &[
            &[Child {
                name: "activeRaceSection",
                wire: Wire::List {
                    value: &Wire::Fixed(1),
                    key: &Wire::Fixed(4),
                    full: &Wire::Seq(&[Wire::Fixed(4), Wire::Fixed(1)]),
                },
            }],
            &[],
        ],
    },
    ClassLayout {
        class: 6234,
        name: "Javelin::InstancedSlayerScriptReplicatedState",
        slots: &[
            &[
                Child {
                    name: "curScriptStateId",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "curScriptId",
                    wire: Wire::Fixed(4),
                },
                Child {
                    name: "spawnedEntityIdsBySpawnerId",
                    wire: Wire::List {
                        value: &Wire::Fixed(8),
                        key: &Wire::Fixed(4),
                        full: &Wire::Seq(&[Wire::Fixed(4), Wire::Fixed(8)]),
                    },
                },
                Child {
                    name: "syncedTimers",
                    wire: Wire::List {
                        value: &Wire::Wide,
                        key: &Wire::Fixed(4),
                        full: &Wire::Seq(&[Wire::Fixed(4), Wire::Fixed(8)]),
                    },
                },
            ],
            &[],
            &[
                Child {
                    name: "scriptTagId",
                    wire: Wire::Fixed(4),
                },
                Child {
                    name: "scriptLocation",
                    wire: Wire::Fixed(12),
                },
                Child {
                    name: "activeTaskId",
                    wire: Wire::Fixed(8),
                },
            ],
            &[],
        ],
    },
    ClassLayout {
        class: 6786,
        name: "MB::EncounterManagerComponentReplicatedState",
        slots: &[
            &[Child {
                name: "status",
                wire: Wire::Fixed(4),
            }],
            &[],
        ],
    },
    ClassLayout {
        class: 6817,
        name: "Javelin::PvPSpectatorCamControllerReplicatedState",
        slots: &[
            &[],
            &[
                Child {
                    name: "enabled",
                    wire: Wire::Fixed(1),
                },
                Child {
                    name: "triggeredViaInteractable",
                    wire: Wire::Fixed(1),
                },
            ],
            &[],
        ],
    },
    ClassLayout {
        class: 6883,
        name: "MB::StealthInvisibilityComponentReplicatedState",
        slots: &[
            &[Child {
                name: "m_isStealthed",
                wire: Wire::Fixed(1),
            }],
            &[],
        ],
    },
    ClassLayout {
        class: 6951,
        name: "Javelin::PointsAccumulatorComponentReplicatedState",
        slots: &[
            &[
                Child {
                    name: "",
                    wire: Wire::Fixed(4),
                },
                Child {
                    name: "",
                    wire: Wire::Fixed(4),
                },
                Child {
                    name: "",
                    wire: Wire::Fixed(8),
                },
            ],
            &[],
        ],
    },
];
