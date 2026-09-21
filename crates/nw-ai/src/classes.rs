// SPDX-License-Identifier: AGPL-3.0-or-later
//! The 77 type UUIDs of the `.behaviortree` ObjectStreams (behavior-tree study, section 1.3).
//!
//! Observed: each class is registered in the client exe's SerializeContext by
//! `FUN_1414169c0(ctx, &out, "Name", uuidPtr, ...)`, with its base in `"BaseClass1"`; the AZ core
//! primitives have their names beside their UUIDs in rdata. The template types (pointers and
//! vectors) are version-5 UUIDs made at run time; `EA418FC5` is built by `FUN_14261e700` as
//! `AZStd::intrusive_ptr` of the `Node` UUID (Observed), the other four are Inferred from the
//! element they hold. Every UUID below occurs in the Server.pak trees (study `stats.txt`).

/// A class UUID as stored in an element (the 16 bytes in text order).
pub type TypeUuid = [u8; 16];

const fn uuid(value: u128) -> TypeUuid {
    value.to_be_bytes()
}

/// The classes of the behavior-tree streams.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Class {
    /// `bool`.
    Bool,
    /// `float`.
    Float,
    /// `int`.
    Int,
    /// `unsigned int`.
    UnsignedInt,
    /// `unsigned char`.
    UnsignedChar,
    /// `unsigned short`.
    UnsignedShort,
    /// `AZ::u64`.
    U64,
    /// `AZStd::string`.
    String,
    /// `AZ::Uuid`.
    Uuid,
    /// `AZ::Crc32`.
    Crc32,
    /// `AZStd::any`.
    Any,
    /// `AZ::Data::Asset`.
    Asset,
    /// `AZStd::intrusive_ptr<Node>`.
    NodePtr,
    /// `AZStd::intrusive_ptr<Tree>` (Inferred).
    TreePtr,
    /// Vector of node pointers (Inferred).
    NodeVector,
    /// `vector<Condition>` (Inferred).
    ConditionVector,
    /// Vector of `TagOperation` (Inferred).
    TagOperationVector,
    /// `Node`, the base of every node.
    Node,
    /// `Decorator` (base).
    Decorator,
    /// `Composite` (base).
    Composite,
    /// `Tree`.
    Tree,
    /// `BehaviorParameter`.
    BehaviorParameter,
    /// `Condition`.
    Condition,
    /// `ConditionGroup`.
    ConditionGroup,
    /// `PositionType`.
    PositionType,
    /// `Parameters` (`SelectPositionParams`).
    Parameters,
    /// `NavRayCastHelper`.
    NavRayCastHelper,
    /// `ActionTargetHelper`.
    ActionTargetHelper,
    /// `TargetSelectionLimits`.
    TargetSelectionLimits,
    /// `TagOperation`.
    TagOperation,
    /// `TreeReference`.
    TreeReference,
    /// `Sequencer`.
    Sequencer,
    /// `PermissiveSequencer`.
    PermissiveSequencer,
    /// `Selector`.
    Selector,
    /// `UtilitySelector`.
    UtilitySelector,
    /// `Parallel`.
    Parallel,
    /// `ForEachTargetDecorator` (a composite: it has `Children`).
    ForEachTargetDecorator,
    /// `BlackboardCondition`.
    BlackboardCondition,
    /// `DistanceCondition`.
    DistanceCondition,
    /// `CooldownDecorator`.
    CooldownDecorator,
    /// `IsCooldownActiveCondition`.
    IsCooldownActiveCondition,
    /// `MannequinTagCondition`.
    MannequinTagCondition,
    /// `SetMannequinTag` (a decorator: it has `Child`).
    SetMannequinTag,
    /// `ActionTargetTrackerDecorator`.
    ActionTargetTrackerDecorator,
    /// `Timer`.
    Timer,
    /// `FrequencyLimiter`.
    FrequencyLimiter,
    /// `VariableExistsCondition`.
    VariableExistsCondition,
    /// `CountLimiter`.
    CountLimiter,
    /// `LockTargetSelection`.
    LockTargetSelection,
    /// `RandomChanceCondition`.
    RandomChanceCondition,
    /// `StatusEffectDecorator`.
    StatusEffectDecorator,
    /// `EncounterEventDecorator`.
    EncounterEventDecorator,
    /// `AlwaysSucceed`.
    AlwaysSucceed,
    /// `VariableMonitor`.
    VariableMonitor,
    /// `VisibilityRaycast`.
    VisibilityRaycast,
    /// `EntityQueryCondition`.
    EntityQueryCondition,
    /// `Repeater`.
    Repeater,
    /// `ManualTargeting`.
    ManualTargeting,
    /// `CAGEAction`.
    CageAction,
    /// `NavigationAction`.
    NavigationAction,
    /// `StrafeAction`.
    StrafeAction,
    /// `SetCooldownAction`.
    SetCooldownAction,
    /// `SetBlackboardFactAction`.
    SetBlackboardFactAction,
    /// `SetBlackboardFactOnRemoteEntityAction`.
    SetBlackboardFactOnRemoteEntityAction,
    /// `TriggerEntity`.
    TriggerEntity,
    /// `TriggerRemoteEntity`.
    TriggerRemoteEntity,
    /// `SetBeamAttackActive`.
    SetBeamAttackActive,
    /// `ExecuteSpawn`.
    ExecuteSpawn,
    /// `SelectPositionAction`.
    SelectPositionAction,
    /// `FakeAction`.
    FakeAction,
    /// `CompleteOrder`.
    CompleteOrder,
    /// `AssignOrder`.
    AssignOrder,
    /// `CountEntitiesWithTag`.
    CountEntitiesWithTag,
    /// `Delay`.
    Delay,
    /// `SelectWaypointAction`.
    SelectWaypointAction,
    /// `SpawnProjectile`.
    SpawnProjectile,
    /// `UpdateNWTags`.
    UpdateNwTags,
}

/// Every class with its UUID (study `typenames.tsv` and `stats.txt`).
pub const CLASSES: [(Class, TypeUuid); 77] = [
    (Class::Bool, uuid(0xA0CA880C_AFE4_43CB_926C_59AC48496112)),
    (Class::Float, uuid(0xEA2C3E90_AFBE_44D4_A90D_FAAF79BAF93D)),
    (Class::Int, uuid(0x72039442_EB38_4D42_A1AD_CB68F7E0EEF6)),
    (
        Class::UnsignedInt,
        uuid(0x43DA906B_7DEF_4CA8_9790_854106D3F983),
    ),
    (
        Class::UnsignedChar,
        uuid(0x72B9409A_7D1A_4831_9CFE_FCB3FADD3426),
    ),
    (
        Class::UnsignedShort,
        uuid(0xECA0B403_C4F8_4B86_95FC_81688D046E40),
    ),
    (Class::U64, uuid(0xD6597933_47CD_4FC8_B911_63F3E2B0993A)),
    (Class::String, uuid(0x03AAAB3F_5C47_5A66_9EBC_D5FA4DB353C9)),
    (Class::Uuid, uuid(0xE152C105_A133_4D03_BBF8_3D4B2FBA3E2A)),
    (Class::Crc32, uuid(0x9F4E062E_06A0_46D4_85DF_E0DA96467D3A)),
    (Class::Any, uuid(0x03924488_C7F4_4D6D_948B_ABC2D1AE2FD3)),
    (Class::Asset, uuid(0x77A19D40_8731_4D3C_9041_1B43047366A4)),
    (Class::NodePtr, uuid(0xEA418FC5_62CE_52A8_ABB4_299F072B6629)),
    (Class::TreePtr, uuid(0x9E2CCCEA_8515_5470_9C31_2B72DBFF5D97)),
    (
        Class::NodeVector,
        uuid(0x15B4F50E_8C6E_5262_8555_E181A9B6FFAC),
    ),
    (
        Class::ConditionVector,
        uuid(0xA3BE97B0_BE01_51C4_9717_7CDD03C6C10E),
    ),
    (
        Class::TagOperationVector,
        uuid(0x00CBC0B4_D4D5_5A84_964D_25C0D40506FD),
    ),
    (Class::Node, uuid(0xC5DD7EA9_9644_4965_8E1B_95170FE334F0)),
    (
        Class::Decorator,
        uuid(0x3D533F66_1710_4283_A371_61D758531F21),
    ),
    (
        Class::Composite,
        uuid(0x47656A8C_663D_48AC_97A6_157BAACC3427),
    ),
    (Class::Tree, uuid(0x8F335743_E650_431C_A9F9_26CB9DC57720)),
    (
        Class::BehaviorParameter,
        uuid(0x7F9B6C29_EB63_4B4E_A2B7_B516CC46E4C2),
    ),
    (
        Class::Condition,
        uuid(0x09240109_42B0_4BFD_BA59_9FEDA69055D4),
    ),
    (
        Class::ConditionGroup,
        uuid(0x395D9EB9_0FE1_41C7_907D_D51A1F05FDB0),
    ),
    (
        Class::PositionType,
        uuid(0xD117F50D_6A27_4898_86EB_AB1CF2B2B75B),
    ),
    (
        Class::Parameters,
        uuid(0x69524C86_0664_425D_A064_E53F667CF12C),
    ),
    (
        Class::NavRayCastHelper,
        uuid(0xE9608AAD_F84E_40B4_A561_882BEA4AFC74),
    ),
    (
        Class::ActionTargetHelper,
        uuid(0x4F4D1504_4C85_4B10_80CF_F20CFBEE5AB6),
    ),
    (
        Class::TargetSelectionLimits,
        uuid(0xBBDF342C_BB09_49AD_BA03_5D58A448325E),
    ),
    (
        Class::TagOperation,
        uuid(0xEC2F0E2F_6DB9_4112_8D6C_EF998555B406),
    ),
    (
        Class::TreeReference,
        uuid(0xB3B42F29_736D_4DFA_BE0A_4EECA6EF4154),
    ),
    (
        Class::Sequencer,
        uuid(0x700BC247_1325_4F6F_BA9E_3E0211F51341),
    ),
    (
        Class::PermissiveSequencer,
        uuid(0xC5E0950B_D0AC_4529_A555_0328181BACBE),
    ),
    (
        Class::Selector,
        uuid(0x90E3433D_5074_47E7_9CF9_1E2524AA0DD0),
    ),
    (
        Class::UtilitySelector,
        uuid(0x0205ACB6_1F79_48DA_BA79_BFD19264F835),
    ),
    (
        Class::Parallel,
        uuid(0x6D32B085_73FD_4E1E_8DE4_F302C4DA0233),
    ),
    (
        Class::ForEachTargetDecorator,
        uuid(0x7E50312C_B4E0_4CB6_8E60_0FBBF384EC69),
    ),
    (
        Class::BlackboardCondition,
        uuid(0x76F3F6D5_B2DC_47C2_810D_26334639D69A),
    ),
    (
        Class::DistanceCondition,
        uuid(0xD911AF6C_F33D_4C4E_AB4F_7E927CD7103A),
    ),
    (
        Class::CooldownDecorator,
        uuid(0xBDB7CB54_D821_4D03_A06D_1C7B03479B9A),
    ),
    (
        Class::IsCooldownActiveCondition,
        uuid(0xCAA9F736_E1E3_4EDE_937E_6AA43DEAC610),
    ),
    (
        Class::MannequinTagCondition,
        uuid(0xFF0D4A6B_0C2F_43BC_A7D7_FE832B1A8738),
    ),
    (
        Class::SetMannequinTag,
        uuid(0x0B28163F_1D4C_49B2_A769_2CC5F5C5D3ED),
    ),
    (
        Class::ActionTargetTrackerDecorator,
        uuid(0x87DC4473_FF34_4282_BE99_84496962B269),
    ),
    (Class::Timer, uuid(0x024B22DE_A0E5_4166_9C3F_CACA62A00E1A)),
    (
        Class::FrequencyLimiter,
        uuid(0x87CB86B4_6626_4FC2_B4D3_B23C70EDD153),
    ),
    (
        Class::VariableExistsCondition,
        uuid(0xAFAFF68E_1F0E_43A6_9056_C5607745753D),
    ),
    (
        Class::CountLimiter,
        uuid(0x0B1F52D1_1158_44A5_93FA_96FEB1968CBA),
    ),
    (
        Class::LockTargetSelection,
        uuid(0x55F4FD4B_B7B5_494F_95BC_9DFC23E5AE4F),
    ),
    (
        Class::RandomChanceCondition,
        uuid(0x7E3894B2_B5A4_4F86_9573_7A271255CA1B),
    ),
    (
        Class::StatusEffectDecorator,
        uuid(0xFDBCB68F_3447_4C7B_A4A4_91AA925BFEBC),
    ),
    (
        Class::EncounterEventDecorator,
        uuid(0xBF18A7DA_A13B_49AA_B590_FDFF64D7348E),
    ),
    (
        Class::AlwaysSucceed,
        uuid(0x3EEFD1A1_853A_42D5_B6D6_233AC96A7EFC),
    ),
    (
        Class::VariableMonitor,
        uuid(0xE250ADF8_55AD_434E_AB1C_25D87BB9FF31),
    ),
    (
        Class::VisibilityRaycast,
        uuid(0xD14048B4_567C_4B63_8348_93C3D21A5C65),
    ),
    (
        Class::EntityQueryCondition,
        uuid(0x31D73F2C_EF5C_4D22_AE94_C022A29981AD),
    ),
    (
        Class::Repeater,
        uuid(0x1FAE53C5_D702_4D5C_B4ED_794379BE5008),
    ),
    (
        Class::ManualTargeting,
        uuid(0xB43B80EC_6554_447E_A56F_D39B948B8DE4),
    ),
    (
        Class::CageAction,
        uuid(0x0F965D75_65CE_4876_B960_EF10D602EB4D),
    ),
    (
        Class::NavigationAction,
        uuid(0x4AB9C781_242A_4A68_BC8A_B579DC025B98),
    ),
    (
        Class::StrafeAction,
        uuid(0xF7D3AC40_7EBD_4A90_8B3E_249D505C25FC),
    ),
    (
        Class::SetCooldownAction,
        uuid(0x04BEFA18_9B0D_44EF_B16A_15399569EBE0),
    ),
    (
        Class::SetBlackboardFactAction,
        uuid(0x6FE0537D_9078_4D1C_A7FE_375B7554A166),
    ),
    (
        Class::SetBlackboardFactOnRemoteEntityAction,
        uuid(0xCEA85D62_0E2E_42A5_816C_2D86B15C35AD),
    ),
    (
        Class::TriggerEntity,
        uuid(0xAEB42127_DAB4_43D5_BEAE_80191B3369E6),
    ),
    (
        Class::TriggerRemoteEntity,
        uuid(0x76D6B4D0_CF9E_4928_9554_57D770137184),
    ),
    (
        Class::SetBeamAttackActive,
        uuid(0xF053301D_BA22_4333_86F7_63E114433C78),
    ),
    (
        Class::ExecuteSpawn,
        uuid(0x1D5CAD0E_8164_4486_8D11_23A719659F60),
    ),
    (
        Class::SelectPositionAction,
        uuid(0x32FDDC4B_F7C8_4774_8850_4239D9D5BD37),
    ),
    (
        Class::FakeAction,
        uuid(0x9BF01D9C_9BB2_4609_ABC7_6F81AB2473BE),
    ),
    (
        Class::CompleteOrder,
        uuid(0x02F823C9_CC59_4E17_A2B8_43AD7AE7A986),
    ),
    (
        Class::AssignOrder,
        uuid(0xEC96A45F_4A3A_4DE0_A2EA_7EA3E8556492),
    ),
    (
        Class::CountEntitiesWithTag,
        uuid(0xD3830552_9C9E_426D_99AE_333858B50B4F),
    ),
    (Class::Delay, uuid(0x1FE0DC92_35CF_4261_BDE7_CC3985DFC4BE)),
    (
        Class::SelectWaypointAction,
        uuid(0xB7EDD82E_5E0B_4D26_9BD5_CC560FA4BD93),
    ),
    (
        Class::SpawnProjectile,
        uuid(0x6F319EF4_ABB6_42C6_A398_05CBF7FC752A),
    ),
    (
        Class::UpdateNwTags,
        uuid(0x1C8C9A24_B4DF_4D5C_B819_3493CA4BD560),
    ),
];

/// The class of a stored type UUID.
pub fn class_of(type_id: &TypeUuid) -> Option<Class> {
    CLASSES
        .iter()
        .find(|(_, known)| known == type_id)
        .map(|(class, _)| *class)
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn every_class_has_its_own_uuid() {
        let uuids: HashSet<TypeUuid> = CLASSES.iter().map(|(_, uuid)| *uuid).collect();
        let classes: HashSet<Class> = CLASSES.iter().map(|(class, _)| *class).collect();
        assert_eq!(uuids.len(), CLASSES.len());
        assert_eq!(classes.len(), CLASSES.len());
    }

    #[test]
    fn the_node_uuid_reads_in_text_order() {
        let node = [
            0xC5, 0xDD, 0x7E, 0xA9, 0x96, 0x44, 0x49, 0x65, 0x8E, 0x1B, 0x95, 0x17, 0x0F, 0xE3,
            0x34, 0xF0,
        ];
        assert_eq!(class_of(&node), Some(Class::Node));
        assert_eq!(class_of(&[0; 16]), None);
    }
}
