// SPDX-License-Identifier: AGPL-3.0-or-later
//! The barber (6420 `PurchaseAppearanceChange`): a player spends one barbershop token of its
//! account on a new look (store tokens note, 2026-11-11).
//!
//! Observed (`buy_barbertoken_and_change_style`, ten calls; `Hive_Transmog`, one): the call holds
//! the whole new look, the gender too. Retail answers 5936 on the player's `EntitlementComponent`
//! with a debit of one `Ent_Barbershop_Token` (status 4) 0.2 s later; then, in one millisecond,
//! 3585 and 4147 (the title list, see below), 6855 on the call's own header and 1285 with the
//! token's new count; the next own bundle, about 50 ms later, carries the own appearance change
//! (only what changed, [`PlayerAppearanceState::change`]).
//!
//! This server: the look must be one of the character-creation lists ([`look_in_lists`]); the
//! loop takes one token from the account store and saves the look (it is the saved look of every
//! later login, [`nw_login::look::saved_look`]); then the player gets 6855 and 1285, its next own
//! bundle the change, the players who see it the same change ([`super::replica`]) and the social
//! service the new icon. Not sent: retail's 3585 and 4147 after the change. They follow every
//! entitlement change on retail (the 898 purchase and the 5818 token uses too) and re-send the
//! title list (Inferred: a title check, as some titles come from entitlements); no title of this
//! server comes from an entitlement, and the client asks for its titles itself (4143).
//!
//! A refusal gets 6436 `OnPurchaseError` on the call's own header
//! ([`AppearancePurchaseErrorMsg`]; Observed: the client's one other answer to the barber call,
//! header only). A look outside the lists is refused before any token use, so it gets 6436 alone.
//! A token use that fails (no token, or the account store failed) first closes its debit request
//! with 3113 ([`super::store_purchase`]), then 6436. No capture holds a refused barber call, so
//! this order is Inferred.

use bevy_ecs::prelude::Entity;
use nw_db::{AccountConsumable, DbError};
use nw_login::look::look_in_lists;
use nw_messages::ServerMessage;
use nw_messages::appearance::{
    AppearanceChangeRequest, AppearancePurchaseErrorMsg, AppearancePurchasedMsg,
    PlayerAppearanceState, next_change_flag,
};
use uuid::Uuid;

use super::economy::count_call;
use super::store_purchase::{GrantKind, NO_PRICE, REASON_NO_TOKEN, REASON_NOT_SAVED, StoreGrant};
use super::{Player, SimWorld, send_now};

/// The consumable the barber spends (Observed: the debit of every retail barber call).
const BARBERSHOP_TOKEN: &str = "Ent_Barbershop_Token";

impl SimWorld {
    /// Player `entity` buys the look of `request` (6420): a look of the character-creation lists
    /// gets 5936 now and a token use for the loop; another look is refused with 6436.
    pub(super) fn barber_change(&mut self, entity: Entity, request: &AppearanceChangeRequest) {
        let Some(player) = self.world.get::<Player>(entity) else {
            return;
        };
        if !look_in_lists(&request.look) {
            count_call("barber", false);
            tracing::info!(player = player.number, look = ?request.look.choices(), "barber change refused: a choice is not in its list");
            let connection = player.connection.clone();
            send_now(
                &mut self.combat_messages,
                &connection,
                &[refused(request.header)],
            );
            return;
        }
        let kind = GrantKind::Barber {
            character: player.character,
            look: request.look,
            call: request.header,
        };
        // Observed: a new UUID v4 as the request id of each retail debit.
        let request_id = Uuid::new_v4().into_bytes();
        self.start_store_request(
            entity,
            BARBERSHOP_TOKEN.to_owned(),
            NO_PRICE,
            kind,
            None,
            request_id,
        );
    }

    /// The loop spent a token for the barber change `grant` (`answer`). With the token spent,
    /// the player, if it is still on this sim, takes the look: 6855 and 1285 go now, the change
    /// with its next own bundle and to the players who see it. Returns whether the look changed.
    /// An account without a token, or a failed use, counts as refused, logs and gets 3113 and
    /// 6436.
    pub(super) fn barber_paid(
        &mut self,
        grant: &StoreGrant,
        answer: Result<Option<AccountConsumable>, DbError>,
    ) -> bool {
        let GrantKind::Barber { look, call, .. } = grant.kind else {
            return false;
        };
        let held = match answer {
            Ok(Some(held)) => held,
            Ok(None) => {
                count_call("barber", false);
                tracing::info!(account = %grant.account, "barber change refused: the account holds no barbershop token");
                self.store_request_failed(grant, REASON_NO_TOKEN, Some(refused(call)));
                return false;
            }
            Err(error) => {
                count_call("barber", false);
                tracing::error!(%error, account = %grant.account, "barber token not spent");
                self.store_request_failed(grant, REASON_NOT_SAVED, Some(refused(call)));
                return false;
            }
        };
        count_call("barber", true);
        let Some(entity) = self.player_by_number(grant.number) else {
            return false;
        };
        let done = self.token_done(grant, &held);
        let Some(mut player) = self.world.get_mut::<Player>(entity) else {
            return false;
        };
        let before = player.look.unwrap_or_default();
        player.look_flag = next_change_flag(player.look_flag);
        player.look_change = Some(PlayerAppearanceState::change(
            &before,
            &look,
            player.look_flag,
        ));
        player.look = Some(look);
        let connection = player.connection.clone();
        let purchased = ServerMessage::AppearancePurchased(AppearancePurchasedMsg { header: call });
        send_now(&mut self.combat_messages, &connection, &[purchased, done]);
        true
    }
}

/// 6436: the barber call on `header` is refused.
const fn refused(header: nw_messages::FacetHeader) -> ServerMessage {
    ServerMessage::AppearancePurchaseError(AppearancePurchaseErrorMsg { header })
}

#[cfg(test)]
mod tests {
    use nw_messages::appearance::{LOOK_CHANGED, LOOK_UNCHANGED};
    use nw_messages::store::{DEBIT_REQUEST_STATUS, RequestInitializedMsg, RequestSuccessMsg};
    use nw_messages::{FacetHeader, PlayerLook};
    use nw_reflect::Envelope;

    use super::*;
    use crate::world::store_purchase::GrantAnswer;
    use crate::world::tests::{NOW, SERVER_TIME, admit_at, connection, world_with_spawns};

    const PLAYER: u64 = 7;
    /// A made-up header of the barber call.
    const CALL: FacetHeader = FacetHeader {
        target: 0x0102,
        entity: 0x0304,
    };
    /// The first look of `buy_barbertoken_and_change_style`, and the look of its first change.
    const FIRST_LOOK: [u8; 12] = [0, 30, 0, 0, 0, 26, 27, 0, 0, 0, 0, 4];
    const CHANGED_LOOK: [u8; 12] = [1, 28, 0, 0, 0, 26, 27, 0, 0, 0, 0, 4];

    fn tokens_left(amount: i64) -> AccountConsumable {
        AccountConsumable {
            entitlement: BARBERSHOP_TOKEN.to_owned(),
            amount,
            purchases: 1,
            updated_at_ms: 0,
        }
    }

    fn sent(world: &mut SimWorld) -> Vec<ServerMessage> {
        std::mem::take(&mut world.combat_messages)
            .into_iter()
            .filter(|out| out.connection == connection(PLAYER))
            .flat_map(|out| out.envelopes)
            .filter_map(|bytes| {
                let envelope = Envelope::decode(&bytes).ok()?;
                ServerMessage::decode(envelope.type_index()?, &envelope.body)?.ok()
            })
            .collect()
    }

    fn barber_world() -> SimWorld {
        let mut world = world_with_spawns(Vec::new());
        let mut admit = admit_at(PLAYER, 0.0, true);
        admit.look = Some(FIRST_LOOK);
        world.admit(&admit, NOW, SERVER_TIME).expect("admit");
        world.combat_messages.clear();
        world
    }

    fn ask(world: &mut SimWorld, look: [u8; 12]) -> Vec<StoreGrant> {
        let entity = world.player_by_number(PLAYER).expect("player");
        world.barber_change(
            entity,
            &AppearanceChangeRequest {
                header: CALL,
                look: PlayerLook::of_choices(look),
            },
        );
        world.take_store_grants()
    }

    #[test]
    fn a_change_spends_a_token_then_answers_and_changes_the_look() {
        let mut world = barber_world();
        let [grant] = &ask(&mut world, CHANGED_LOOK)[..] else {
            panic!("one token use");
        };
        assert_eq!(grant.entitlement, BARBERSHOP_TOKEN);
        let started = sent(&mut world);
        let [ServerMessage::RequestInitialized(RequestInitializedMsg(debit))] = &started[..] else {
            panic!("5936 first: {started:?}");
        };
        assert_eq!(debit.status, DEBIT_REQUEST_STATUS);
        assert!(world.store_answered(grant, GrantAnswer::Barber(Ok(Some(tokens_left(14))))));
        let answers = sent(&mut world);
        let [
            ServerMessage::AppearancePurchased(purchased),
            ServerMessage::RequestSuccess(RequestSuccessMsg(done)),
        ] = &answers[..]
        else {
            panic!("6855 then 1285: {answers:?}");
        };
        assert_eq!(purchased.header, CALL);
        assert_eq!(done.entitlements[0].total, 14);
        let entity = world.player_by_number(PLAYER).expect("player");
        let player = world.world.get::<Player>(entity).expect("player");
        assert_eq!(player.look, Some(PlayerLook::of_choices(CHANGED_LOOK)));
        assert_eq!(player.look_flag, LOOK_CHANGED);
        let change = player.look_change.expect("a change for the own bundle");
        assert_eq!(change.choices[0], Some(1));
        assert_eq!(change.choices[1], Some(28));
        assert_eq!(change.choices[2], None, "the skin tone did not change");
        assert_eq!(change.change_flag, Some(LOOK_CHANGED));
    }

    #[test]
    fn no_token_or_a_look_outside_the_lists_is_refused_and_changes_nothing() {
        let mut world = barber_world();
        let mut outside = CHANGED_LOOK;
        outside[3] = u8::MAX;
        assert!(ask(&mut world, outside).is_empty(), "no token is spent");
        let refused = sent(&mut world);
        let [ServerMessage::AppearancePurchaseError(error)] = &refused[..] else {
            panic!("6436 alone: {refused:?}");
        };
        assert_eq!(error.header, CALL);
        let [grant] = &ask(&mut world, CHANGED_LOOK)[..] else {
            panic!("one token use");
        };
        let started = sent(&mut world);
        let [ServerMessage::RequestInitialized(RequestInitializedMsg(debit))] = &started[..] else {
            panic!("5936 first: {started:?}");
        };
        assert!(!world.store_answered(grant, GrantAnswer::Barber(Ok(None))));
        let answers = sent(&mut world);
        let [
            ServerMessage::RequestFail(failed),
            ServerMessage::AppearancePurchaseError(error),
        ] = &answers[..]
        else {
            panic!("3113 then 6436: {answers:?}");
        };
        assert_eq!(
            &failed.request, debit,
            "the 3113 closes the debit of the 5936"
        );
        assert_eq!(failed.reason, REASON_NO_TOKEN.as_bytes());
        assert_eq!(error.header, CALL);
        let entity = world.player_by_number(PLAYER).expect("player");
        let player = world.world.get::<Player>(entity).expect("player");
        assert_eq!(player.look, Some(PlayerLook::of_choices(FIRST_LOOK)));
        assert_eq!(player.look_flag, LOOK_UNCHANGED);
    }
}
