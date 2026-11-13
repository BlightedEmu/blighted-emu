// SPDX-License-Identifier: AGPL-3.0-or-later
//! Players' raids (world migration 0044): the raids that groups grew into by the convert-to-raid
//! vote, stored so that they stay across a restart of the social service, as groups do.
//!
//! The social service saves a raid whole at each change (its row, then every member row again)
//! and deletes it when it ends. The member rows keep the join order (`seat`), each member's raid
//! group, its raid permission and the order the assistants got theirs.

use sqlx::PgPool;
use uuid::Uuid;

use crate::error::DbError;
use crate::social_graph::narrow;

/// The client's `eRaidPermission` of a member that is neither leader nor assistant.
const PERMISSION_NORMAL: i16 = 0;
/// The client's `eRaidPermission` of an assistant.
const PERMISSION_ASSISTANT: i16 = 1;
/// The client's `eRaidPermission` of the leader.
const PERMISSION_LEADER: i16 = 2;
/// Field names of the range errors.
const RAID_FIELD: &str = "player_raid.raid_id";
const SEAT_FIELD: &str = "player_raid_member.seat";
const GROUP_FIELD: &str = "player_raid_member.raid_group";
const RANK_FIELD: &str = "player_raid_member.assistant_rank";

/// A stored players' raid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerRaidRecord {
    /// The raid entity id.
    pub entity: Uuid,
    /// The raid id.
    pub raid: u64,
    /// The four raid group ids, in raid group order.
    pub groups: Vec<Uuid>,
    /// The id of the raid's voice channel.
    pub voice_channel: String,
    /// The members and their raid groups, in join order.
    pub seats: Vec<(Uuid, u8)>,
    /// The leader.
    pub leader: Option<Uuid>,
    /// The assistants, in the order they got the permission.
    pub assistants: Vec<Uuid>,
}

impl PlayerRaidRecord {
    /// The stored permission of `member`.
    fn permission_of(&self, member: Uuid) -> i16 {
        if self.leader == Some(member) {
            PERMISSION_LEADER
        } else if self.assistants.contains(&member) {
            PERMISSION_ASSISTANT
        } else {
            PERMISSION_NORMAL
        }
    }
}

/// Stores `raid` whole: its row, and its members in place of the stored ones.
///
/// # Errors
///
/// Returns [`DbError::Database`] when PostgreSQL fails, and [`DbError::OutOfRange`] for a raid id
/// or a seat that does not fit its column.
pub async fn save_player_raid(world: &PgPool, raid: &PlayerRaidRecord) -> Result<(), DbError> {
    let raid_id: i64 =
        i64::try_from(raid.raid).map_err(|_| DbError::OutOfRange { field: RAID_FIELD })?;
    let mut transaction = world.begin().await?;
    sqlx::query(
        "INSERT INTO player_raid (raid_entity, raid_id, group_ids, voice_channel) \
         VALUES ($1, $2, $3, $4) ON CONFLICT (raid_entity) DO NOTHING",
    )
    .bind(raid.entity)
    .bind(raid_id)
    .bind(&raid.groups)
    .bind(&raid.voice_channel)
    .execute(&mut *transaction)
    .await?;
    sqlx::query("DELETE FROM player_raid_member WHERE raid_entity = $1")
        .bind(raid.entity)
        .execute(&mut *transaction)
        .await?;
    for (seat, (member, group)) in raid.seats.iter().enumerate() {
        let seat: i16 =
            i16::try_from(seat).map_err(|_| DbError::OutOfRange { field: SEAT_FIELD })?;
        let rank = raid
            .assistants
            .iter()
            .position(|assistant| assistant == member)
            .map(i16::try_from)
            .transpose()
            .map_err(|_| DbError::OutOfRange { field: RANK_FIELD })?;
        sqlx::query(
            "INSERT INTO player_raid_member \
             (character_id, raid_entity, seat, raid_group, permission, assistant_rank) \
             VALUES ($1, $2, $3, $4, $5, $6) \
             ON CONFLICT (character_id) DO UPDATE SET raid_entity = $2, seat = $3, \
             raid_group = $4, permission = $5, assistant_rank = $6",
        )
        .bind(member)
        .bind(raid.entity)
        .bind(seat)
        .bind(i16::from(*group))
        .bind(raid.permission_of(*member))
        .bind(rank)
        .execute(&mut *transaction)
        .await?;
    }
    transaction.commit().await?;
    Ok(())
}

/// Deletes the players' raid of raid entity `entity` and its members.
///
/// # Errors
///
/// Returns [`DbError::Database`] when PostgreSQL fails.
pub async fn delete_player_raid(world: &PgPool, entity: Uuid) -> Result<(), DbError> {
    sqlx::query("DELETE FROM player_raid WHERE raid_entity = $1")
        .bind(entity)
        .execute(world)
        .await?;
    Ok(())
}

/// One member row: raid entity, character, raid group, permission, assistant rank.
type MemberRow = (Uuid, Uuid, i16, i16, Option<i16>);

/// Every stored players' raid with its members in join order.
///
/// # Errors
///
/// Returns [`DbError::Database`] when PostgreSQL fails, and [`DbError::OutOfRange`] for a stored
/// value that does not fit its type.
pub async fn load_player_raids(world: &PgPool) -> Result<Vec<PlayerRaidRecord>, DbError> {
    let mut raids = sqlx::query_as::<_, (Uuid, i64, Vec<Uuid>, String)>(
        "SELECT raid_entity, raid_id, group_ids, voice_channel FROM player_raid ORDER BY raid_id",
    )
    .fetch_all(world)
    .await?
    .into_iter()
    .map(|(entity, raid, groups, voice_channel)| {
        Ok(PlayerRaidRecord {
            entity,
            raid: narrow(raid, RAID_FIELD)?,
            groups,
            voice_channel,
            seats: Vec::new(),
            leader: None,
            assistants: Vec::new(),
        })
    })
    .collect::<Result<Vec<_>, DbError>>()?;
    let members = sqlx::query_as::<_, MemberRow>(
        "SELECT raid_entity, character_id, raid_group, permission, assistant_rank \
         FROM player_raid_member ORDER BY raid_entity, seat",
    )
    .fetch_all(world)
    .await?;
    let mut ranked: Vec<(Uuid, i16, Uuid)> = Vec::new();
    for (entity, character, group, permission, rank) in members {
        let Some(raid) = raids.iter_mut().find(|raid| raid.entity == entity) else {
            continue;
        };
        raid.seats
            .push((character, narrow(i64::from(group), GROUP_FIELD)?));
        if permission == PERMISSION_LEADER {
            raid.leader = Some(character);
        }
        if let Some(rank) = rank {
            ranked.push((entity, rank, character));
        }
    }
    ranked.sort_unstable();
    for (entity, _, character) in ranked {
        if let Some(raid) = raids.iter_mut().find(|raid| raid.entity == entity) {
            raid.assistants.push(character);
        }
    }
    Ok(raids)
}

#[cfg(all(test, feature = "infra"))]
mod infra_tests {
    use super::*;
    use crate::migrate::WORLD_MIGRATOR;
    use crate::test_db::with_test_schema;
    use crate::{NewCharacter, insert_character};

    /// A made-up raid id and voice channel.
    const RAID: u64 = 0x1_0003;
    const CHANNEL: &str = "made-up-raid-channel";
    const GROUPS: usize = 4;

    async fn player(pool: &PgPool, name: &str) -> Uuid {
        let id = Uuid::new_v4();
        insert_character(
            pool,
            &NewCharacter {
                id,
                account_id: Uuid::new_v4(),
                name: name.to_owned(),
                appearance: serde_json::json!({}),
            },
        )
        .await
        .expect("insert character");
        id
    }

    #[tokio::test]
    async fn a_players_raid_is_saved_whole_loaded_in_order_and_deleted() {
        with_test_schema(async |pool| {
            WORLD_MIGRATOR.run(pool).await.expect("migrate");
            let leader = player(pool, "Made Up Leader").await;
            let first = player(pool, "Made Up First").await;
            let second = player(pool, "Made Up Second").await;
            let third = player(pool, "Made Up Third").await;
            let mut raid = PlayerRaidRecord {
                entity: Uuid::new_v4(),
                raid: RAID,
                groups: (0..GROUPS).map(|_| Uuid::new_v4()).collect(),
                voice_channel: CHANNEL.to_owned(),
                seats: vec![(leader, 0), (first, 0), (second, 1), (third, 3)],
                leader: Some(leader),
                assistants: vec![second, first],
            };
            save_player_raid(pool, &raid).await.expect("save");
            assert_eq!(
                load_player_raids(pool).await.expect("load"),
                vec![raid.clone()]
            );

            // A change: the first member moves to group 2 (last in the join order), the third
            // leaves, the second is no assistant any more.
            raid.seats = vec![(leader, 0), (second, 1), (first, 2)];
            raid.assistants = vec![first];
            save_player_raid(pool, &raid).await.expect("save again");
            assert_eq!(
                load_player_raids(pool).await.expect("load again"),
                vec![raid.clone()],
                "the stored members are the raid's now"
            );

            delete_player_raid(pool, raid.entity).await.expect("delete");
            assert!(load_player_raids(pool).await.expect("load").is_empty());
        })
        .await;
    }
}
