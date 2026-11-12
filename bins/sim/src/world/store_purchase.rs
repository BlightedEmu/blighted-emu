// SPDX-License-Identifier: AGPL-3.0-or-later
//! Store purchases (898) and the store requests the loop runs against the account store. While
//! `[shop] enabled` is on, the sim sells each mount and each token pack of the store table: for 0
//! while `[shop] free` is on (retail's last weeks, `mount-menu-buying-all-store-horses`,
//! 2026-11-04: 35 mounts for 0 Marks of Fortune; `buy_barbertoken_and_change_style`: a barbershop
//! token for 0), else for its price from the account's wallet. The sim answers 5936 at once; the
//! loop runs the [`StoreGrant`] (pays and saves the entitlement, or adds the pack's tokens, to the
//! player's account in the global database, in one transaction); then the sim answers 1285 with
//! the entitlement and the wallet (retail: 0.12 s and 0.24 s after the call). The account then
//! holds the mount or the tokens on every sim and every later login, and the store service lists
//! them.
//!
//! The barber ([`super::barber`]) spends a token through the same request: the loop takes one
//! token and saves the new look. A status effect token (5818 `RequestTokenDebit`, the
//! `Ent_StatusEffect_Token_*` boosts) too: the loop takes one token, then the player gets 1285
//! with the count left and holds the token's effect from that time ([`EffectTokens`], kept over
//! a save and a handoff; a token used while its effect is held adds its time, `StackDuration`).
//! Observed (`Playthrough_Part_02`, five uses): 5936 0.2 s after the call, then 3585, 4147 and
//! 1285 0.4 s after it; the effect's end is 7,200 s after the use (`Token_Gather_1`: call
//! 1792686628775, end 1792693828424 on the server's clock). Inferred: the effect starts once the
//! token is spent. Not sent: 3585 and 4147 (see [`super::barber`]). Unknown: retail's refusal
//! of a use without a token (no capture); this server logs it and sends nothing more.
//!
//! [`EffectTokens`]: nw_content::store::EffectTokens

use bevy_ecs::prelude::Entity;
use nw_common::time::iso8601_millis;
use nw_db::{
    AccountConsumable, AccountEntitlement, DbError, PgPool, Purchase, Wallet, buy_consumable,
    buy_entitlement, save_changed_look, use_consumable,
};
use nw_messages::facet::component;
use nw_messages::store::{
    CONSUMABLE_ENTITLEMENT_KIND, DEBIT_REQUEST_STATUS, DURABLE_ENTITLEMENT_KIND, EntitlementDebit,
    EntitlementRecord, EntitlementRequest, RETAIL_DEBIT_TAIL, RETAIL_REQUEST_STATUS,
    RequestInitializedMsg, RequestSuccessMsg, StorePurchase, WALLET_ENTITLEMENT_KIND,
};
use nw_messages::{FacetHeader, PlayerLook, RAW_ID_LEN, ServerMessage};
use nw_protocol::store_api::{STORE_CURRENCY, STORE_CURRENCY_NAME};
use uuid::Uuid;

use super::economy::count_call;
use super::{Player, SimWorld, send_now};

/// Nanoseconds in a millisecond: a transaction number is the time in nanoseconds (retail's are
/// 19 digits).
const NANOS_PER_MILLI: u64 = 1_000_000;
/// The amount of a mount entitlement.
const ONE: u32 = 1;
/// Tokens one barber change or one status effect token use spends (Observed: the count went
/// down by one at each of the 11 retail changes; every retail debit's tail holds the amount 1).
const TOKENS_PER_USE: i64 = 1;
/// The price of a use of a token: it is paid with the token.
pub(super) const NO_PRICE: u32 = 0;

/// What a store request does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GrantKind {
    /// Buys a store mount.
    Mount,
    /// Buys a token pack of `amount` tokens.
    Token {
        /// Tokens per purchase.
        amount: u32,
    },
    /// Spends one token on a barber change of character `character` to `look`; `call` is the
    /// header of the barber call, which its answer 6855 carries.
    Barber {
        /// The player's character.
        character: Uuid,
        /// The look it buys.
        look: PlayerLook,
        /// The header of the 6420 call.
        call: FacetHeader,
    },
    /// Spends one status effect token (5818); the player then holds `effect`.
    StatusEffectToken {
        /// The `StatusID` the token starts.
        effect: String,
    },
}

/// A store request that the loop runs against the account store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreGrant {
    /// The player's entity number.
    pub number: u64,
    /// The player's account.
    pub account: Uuid,
    /// The entitlement it buys or spends.
    pub entitlement: String,
    /// Its price in Marks of Fortune: 0 in a free store and for a use.
    pub price: u32,
    /// What it does.
    pub kind: GrantKind,
    /// The header of the answers: the purchase call's own, or the player's
    /// `EntitlementComponent` for a use.
    header: FacetHeader,
    /// The request id: the purchase call's, or a new one for a use.
    request: [u8; RAW_ID_LEN],
    /// The transaction number of the answers.
    transaction: Vec<u8>,
}

/// What the account store did with a [`StoreGrant`].
#[derive(Debug)]
pub enum GrantAnswer {
    /// The mount purchase.
    Mount(Result<Purchase, DbError>),
    /// The token pack purchase.
    Token(Result<Purchase<AccountConsumable>, DbError>),
    /// The token use of a barber change: the tokens left, or `None` when the account held none.
    Barber(Result<Option<AccountConsumable>, DbError>),
    /// The use of a status effect token: the tokens left, or `None` when the account held none.
    StatusEffectToken(Result<Option<AccountConsumable>, DbError>),
}

impl StoreGrant {
    /// The answer to the grant's request that lists `entitlements`: a purchase (Observed, status
    /// 2, no debit, done), or a use (Observed: status 4, the debit of one token; not done for the
    /// barber, done for a status effect token, `Playthrough_Part_02`).
    fn answer(&self, entitlements: Vec<EntitlementRecord>) -> EntitlementRequest {
        let barber = matches!(self.kind, GrantKind::Barber { .. });
        let use_of_token = barber || matches!(self.kind, GrantKind::StatusEffectToken { .. });
        EntitlementRequest {
            header: self.header,
            request: self.request,
            transaction: self.transaction.clone(),
            status: if use_of_token {
                DEBIT_REQUEST_STATUS
            } else {
                RETAIL_REQUEST_STATUS
            },
            debits: if use_of_token {
                vec![EntitlementDebit {
                    entitlement: self.entitlement.as_bytes().to_vec(),
                    tail: RETAIL_DEBIT_TAIL,
                }]
            } else {
                Vec::new()
            },
            entitlements,
            done: !barber,
        }
    }

    /// 5936: the request is taken.
    pub(super) fn started(&self) -> ServerMessage {
        ServerMessage::RequestInitialized(RequestInitializedMsg(self.answer(Vec::new())))
    }

    /// 1285: the request is done, with `entitlements`.
    fn done(&self, entitlements: Vec<EntitlementRecord>) -> ServerMessage {
        ServerMessage::RequestSuccess(RequestSuccessMsg(self.answer(entitlements)))
    }

    /// Runs the request against the global database `global` (and, for a barber change, saves
    /// the look in the world database `world`). A look that does not save after the token was
    /// spent logs: the player keeps the look until it leaves.
    pub async fn run(self, global: &PgPool, world: &PgPool) -> (Self, GrantAnswer) {
        let price = i64::from(self.price);
        let answer = match &self.kind {
            GrantKind::Mount => GrantAnswer::Mount(
                buy_entitlement(global, self.account, &self.entitlement, price).await,
            ),
            GrantKind::Token { amount } => GrantAnswer::Token(
                buy_consumable(
                    global,
                    self.account,
                    &self.entitlement,
                    i64::from(*amount),
                    price,
                )
                .await,
            ),
            GrantKind::Barber {
                character, look, ..
            } => {
                let used = self.use_token(global).await;
                if matches!(used, Ok(Some(_)))
                    && let Err(error) = save_changed_look(world, *character, &look.choices()).await
                {
                    tracing::error!(%error, %character, "barber look not saved");
                }
                GrantAnswer::Barber(used)
            }
            GrantKind::StatusEffectToken { .. } => {
                GrantAnswer::StatusEffectToken(self.use_token(global).await)
            }
        };
        (self, answer)
    }

    /// Takes one of the grant's tokens from the account in the global database `global`.
    async fn use_token(&self, global: &PgPool) -> Result<Option<AccountConsumable>, DbError> {
        use_consumable(global, self.account, &self.entitlement, TOKENS_PER_USE).await
    }
}

/// ISO 8601 text of Unix milliseconds `ms`.
fn iso_time(ms: i64) -> Vec<u8> {
    iso8601_millis(u64::try_from(ms).unwrap_or_default()).into_bytes()
}

/// A count of the account store as a record amount.
fn record_amount(amount: i64) -> u32 {
    u32::try_from(amount).unwrap_or(u32::MAX)
}

/// The 1285 record of the entitlement `alias` named `name`: this server's entitlement id is its
/// alias.
fn record_of(
    alias: &str,
    name: &str,
    kind: u32,
    amount: u32,
    instances: Vec<Vec<u8>>,
    updated_ms: i64,
) -> EntitlementRecord {
    let balance = if kind == DURABLE_ENTITLEMENT_KIND {
        0
    } else {
        amount
    };
    EntitlementRecord {
        id: alias.as_bytes().to_vec(),
        alias: alias.as_bytes().to_vec(),
        name: name.as_bytes().to_vec(),
        kind,
        amounts: [balance, 0, 0],
        total: amount,
        word_a: 0,
        instances,
        word_b: 0,
        updated: iso_time(updated_ms),
        byte_c: 0,
    }
}

/// The 1285 record of the durable entitlement `held`, named `name`.
fn record(held: &AccountEntitlement, name: &str) -> EntitlementRecord {
    record_of(
        &held.entitlement,
        name,
        DURABLE_ENTITLEMENT_KIND,
        ONE,
        vec![held.instance.to_string().into_bytes()],
        held.granted_at_ms,
    )
}

/// The 1285 record of the consumable `held`, named `name` (Observed: kind 1, its count as the
/// free amount and the total, no instance).
fn consumable_record(held: &AccountConsumable, name: &str) -> EntitlementRecord {
    record_of(
        &held.entitlement,
        name,
        CONSUMABLE_ENTITLEMENT_KIND,
        record_amount(held.amount),
        Vec::new(),
        held.updated_at_ms,
    )
}

/// The 1285 record of `wallet`: every Mark of this server is free (none is bought with money).
fn wallet_record(wallet: Wallet) -> EntitlementRecord {
    record_of(
        STORE_CURRENCY,
        STORE_CURRENCY_NAME,
        WALLET_ENTITLEMENT_KIND,
        record_amount(wallet.marks),
        Vec::new(),
        wallet.updated_at_ms,
    )
}

impl SimWorld {
    /// The next transaction number: the time in nanoseconds, above the last one.
    fn next_transaction(&mut self) -> Vec<u8> {
        let transaction = (self.last_now_ms.saturating_mul(NANOS_PER_MILLI))
            .max(self.economy.last_transaction.saturating_add(1));
        self.economy.last_transaction = transaction;
        transaction.to_string().into_bytes()
    }

    /// Player `entity` starts the store request `kind` for `entitlement` at `price`, answered
    /// on `header` with the request id `request`: 5936 goes now and the loop runs the request.
    pub(super) fn start_store_request(
        &mut self,
        entity: Entity,
        entitlement: String,
        price: u32,
        kind: GrantKind,
        header: Option<FacetHeader>,
        request: [u8; RAW_ID_LEN],
    ) {
        let Some(player) = self.world.get::<Player>(entity) else {
            return;
        };
        let (number, account, connection) =
            (player.number, player.account, player.connection.clone());
        let grant = StoreGrant {
            number,
            account,
            entitlement,
            price,
            kind,
            header: header.unwrap_or_else(|| FacetHeader::of(component::ENTITLEMENT, number)),
            request,
            transaction: self.next_transaction(),
        };
        send_now(&mut self.combat_messages, &connection, &[grant.started()]);
        self.economy.grants.push(grant);
    }

    /// Player `entity` buys the offer of `purchase` (898). While the store sells, an offer of the
    /// store table (its id is the entitlement: a mount, or a token pack) gets 5936 now and a
    /// grant for the loop, at its price. A closed store or another offer is refused with no
    /// answer (retail's refusal message 3113 `OnRequestFail` has no capture; its layout is
    /// Unknown).
    pub(super) fn store_purchase(
        &mut self,
        entity: Entity,
        header: FacetHeader,
        purchase: &StorePurchase,
    ) {
        let offer = String::from_utf8_lossy(&purchase.offer).into_owned();
        if !self.content.shop.enabled {
            count_call("store_purchase", false);
            tracing::info!(offer, "store purchase refused: the store is closed");
            return;
        }
        let store = &self.content.store;
        let shop = &self.content.shop;
        let (kind, price) = if store.mount_of(&offer).is_some() {
            (GrantKind::Mount, shop.price(&offer))
        } else if let Some(token) = store.token_of(&offer) {
            (
                GrantKind::Token {
                    amount: token.amount,
                },
                shop.cost(token.list_price),
            )
        } else {
            count_call("store_purchase", false);
            tracing::info!(offer, "store purchase refused: no such store offer");
            return;
        };
        self.start_store_request(
            entity,
            offer,
            price,
            kind,
            Some(header),
            purchase.transaction,
        );
    }

    /// The store requests that the loop runs now.
    pub fn take_store_grants(&mut self) -> Vec<StoreGrant> {
        std::mem::take(&mut self.economy.grants)
    }

    /// The loop ran `grant` (`answer`). Returns whether the player's look changed (its social
    /// view changed with it).
    pub fn store_answered(&mut self, grant: &StoreGrant, answer: GrantAnswer) -> bool {
        match answer {
            GrantAnswer::Mount(answer) => {
                self.store_granted(grant, answer);
                false
            }
            GrantAnswer::Token(answer) => {
                self.token_granted(grant, answer);
                false
            }
            GrantAnswer::Barber(answer) => self.barber_paid(grant, answer),
            GrantAnswer::StatusEffectToken(answer) => {
                self.status_token_used(grant, answer);
                false
            }
        }
    }

    /// Player `entity` spends one status effect token of `entitlement` (5818): a token the data
    /// names ([`EffectTokens`]) gets 5936 now and a token use for the loop; another entitlement
    /// is refused (logged).
    ///
    /// [`EffectTokens`]: nw_content::store::EffectTokens
    pub(super) fn token_debit(&mut self, entity: Entity, entitlement: &[u8]) {
        let entitlement = String::from_utf8_lossy(entitlement).into_owned();
        let Some(effect) = self
            .content
            .effect_tokens
            .effect_of(&entitlement)
            .map(str::to_owned)
        else {
            count_call("token_debit", false);
            tracing::info!(entitlement, "token use refused: no status effect token");
            return;
        };
        // Observed: a new UUID v4 as the request id of each retail debit.
        let request_id = Uuid::new_v4().into_bytes();
        let kind = GrantKind::StatusEffectToken { effect };
        self.start_store_request(entity, entitlement, NO_PRICE, kind, None, request_id);
    }

    /// The loop spent a status effect token for `grant` (`answer`): the player, if it is still
    /// on this sim, gets 1285 with the count left and holds the token's effect from now. An
    /// account without a token, or a failed use, counts as refused and logs.
    fn status_token_used(
        &mut self,
        grant: &StoreGrant,
        answer: Result<Option<AccountConsumable>, DbError>,
    ) {
        let GrantKind::StatusEffectToken { effect } = &grant.kind else {
            return;
        };
        let held = match answer {
            Ok(Some(held)) => held,
            Ok(None) => {
                count_call("token_debit", false);
                tracing::info!(account = %grant.account, entitlement = grant.entitlement, "token use refused: the account holds none");
                return;
            }
            Err(error) => {
                count_call("token_debit", false);
                tracing::error!(%error, account = %grant.account, entitlement = grant.entitlement, "token not spent");
                return;
            }
        };
        count_call("token_debit", true);
        let Some(entity) = self.player_by_number(grant.number) else {
            return;
        };
        let done = self.token_done(grant, &held);
        let server_ms = self.server_ms;
        let content = std::sync::Arc::clone(&self.content);
        let Some(mut player) = self.world.get_mut::<Player>(entity) else {
            return;
        };
        let number = player.number;
        let added = player.status_effects.add_token(
            &content.status_effects,
            &grant.entitlement,
            effect,
            number,
            server_ms,
        );
        tracing::debug!(player = number, effect, added, "token effect");
        let connection = player.connection.clone();
        send_now(&mut self.combat_messages, &connection, &[done]);
    }

    /// The wallet of a purchase `answer`, and what the account holds of the offer; `None` (the
    /// call counts as refused and logs) for a wallet below the price or a failed save: the
    /// buyer holds nothing new, and its client waits (retail's refusal of a store purchase is
    /// Unknown: no capture has one).
    fn paid<H>(grant: &StoreGrant, answer: Result<Purchase<H>, DbError>) -> Option<(H, Wallet)> {
        match answer {
            Ok(Purchase::Bought { held, wallet } | Purchase::AlreadyHeld { held, wallet }) => {
                count_call("store_purchase", true);
                Some((held, wallet))
            }
            Ok(Purchase::TooPoor { wallet }) => {
                count_call("store_purchase", false);
                tracing::info!(
                    entitlement = grant.entitlement,
                    price = grant.price,
                    marks = wallet.marks,
                    "store purchase refused: the wallet holds too few Marks of Fortune"
                );
                None
            }
            Err(error) => {
                count_call("store_purchase", false);
                tracing::error!(%error, entitlement = grant.entitlement, account = %grant.account, "store purchase not saved");
                None
            }
        }
    }

    /// The loop ran the mount purchase `grant` (`answer`): when the account holds the mount
    /// now, the buyer, if it is still on this sim, may use it and gets 1285 with it and the
    /// wallet.
    pub fn store_granted(&mut self, grant: &StoreGrant, answer: Result<Purchase, DbError>) {
        let Some((held, wallet)) = Self::paid(grant, answer) else {
            return;
        };
        let Some(entity) = self.player_by_number(grant.number) else {
            return;
        };
        let name = self.content.store.mount_of(&held.entitlement).map_or_else(
            || held.entitlement.clone(),
            |mount| mount.unlock.name.clone(),
        );
        let Some(mut player) = self.world.get_mut::<Player>(entity) else {
            return;
        };
        player.entitlements.insert(held.entitlement.clone());
        let connection = player.connection.clone();
        let done = grant.done(vec![record(&held, &name), wallet_record(wallet)]);
        send_now(&mut self.combat_messages, &connection, &[done]);
    }

    /// The loop ran the token pack purchase `grant` (`answer`): the buyer, if it is still on
    /// this sim, gets 1285 with its new count and the wallet (Observed: the barbershop token
    /// record with the count 15 after the purchase, and the wallet).
    fn token_granted(
        &mut self,
        grant: &StoreGrant,
        answer: Result<Purchase<AccountConsumable>, DbError>,
    ) {
        let Some((held, wallet)) = Self::paid(grant, answer) else {
            return;
        };
        let Some(connection) = self.connection_of_number(grant.number) else {
            return;
        };
        let done = grant.done(vec![
            consumable_record(&held, self.token_name(&held.entitlement)),
            wallet_record(wallet),
        ]);
        send_now(&mut self.combat_messages, &connection, &[done]);
    }

    /// The name of the consumable `entitlement`: the store table's, else the entitlement.
    pub(super) fn token_name<'a>(&'a self, entitlement: &'a str) -> &'a str {
        self.content
            .store
            .token_of(entitlement)
            .map_or(entitlement, |token| token.token.name.as_str())
    }

    /// The connection of the player numbered `number`, when it is on this sim.
    fn connection_of_number(&mut self, number: u64) -> Option<nw_bus::ConnectionRef> {
        let entity = self.player_by_number(number)?;
        Some(self.world.get::<Player>(entity)?.connection.clone())
    }

    /// 1285 of `grant` with the record of the consumable `held`.
    pub(super) fn token_done(&self, grant: &StoreGrant, held: &AccountConsumable) -> ServerMessage {
        grant.done(vec![consumable_record(
            held,
            self.token_name(&held.entitlement),
        )])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The 1285 of the first retail barber change (`buy_barbertoken_and_change_style`
    /// 1781370830595) with made-up ids: status 4, the debit, the token at 14, not done.
    #[test]
    fn a_use_of_a_token_answers_as_a_retail_debit() {
        let grant = StoreGrant {
            number: 1,
            account: Uuid::from_u128(1),
            entitlement: "Ent_Barbershop_Token".to_owned(),
            price: 0,
            kind: GrantKind::Barber {
                character: Uuid::from_u128(2),
                look: PlayerLook::default(),
                call: FacetHeader {
                    target: 3,
                    entity: 4,
                },
            },
            header: FacetHeader {
                target: 5,
                entity: 4,
            },
            request: [7; RAW_ID_LEN],
            transaction: b"12345678901234567890".to_vec(),
        };
        let held = AccountConsumable {
            entitlement: grant.entitlement.clone(),
            amount: 14,
            purchases: 15,
            updated_at_ms: 1_781_370_830_731,
        };
        let ServerMessage::RequestSuccess(RequestSuccessMsg(done)) =
            grant.done(vec![consumable_record(&held, "Barbershop Token")])
        else {
            panic!("1285");
        };
        assert_eq!(done.status, DEBIT_REQUEST_STATUS);
        assert!(!done.done);
        assert_eq!(done.debits[0].tail, RETAIL_DEBIT_TAIL);
        let [token] = &done.entitlements[..] else {
            panic!("one record");
        };
        assert_eq!(
            (token.kind, token.amounts, token.total),
            (CONSUMABLE_ENTITLEMENT_KIND, [14, 0, 0], 14)
        );
        assert_eq!(token.updated, b"2026-06-13T17:13:50.731Z");
        let ServerMessage::RequestInitialized(RequestInitializedMsg(started)) = grant.started()
        else {
            panic!("5936");
        };
        assert!(started.entitlements.is_empty() && !started.done);
        assert_eq!(started.debits, done.debits);
    }

    #[test]
    fn a_token_pack_is_sold_for_its_cost_and_answers_with_the_count_and_the_wallet() {
        use std::sync::Arc;

        use nw_protocol::store_api::{StoreEntitlement, StoreTable, StoreToken};
        use nw_reflect::Envelope;

        use crate::player_content::PlayerContent;
        use crate::world::tests::{NOW, SERVER_TIME, admit_at, connection, world_with_spawns};

        const PLAYER: u64 = 7;
        const TOKEN: &str = "Ent_Barbershop_Token";
        let mut world = world_with_spawns(Vec::new());
        world.content = Arc::new(PlayerContent {
            store: StoreTable {
                tokens: vec![StoreToken {
                    token: StoreEntitlement {
                        entitlement: TOKEN.to_owned(),
                        name: "Barbershop Token".to_owned(),
                    },
                    product: "StoreItem_Token_Barbershop_X1".to_owned(),
                    product_name: "Barbershop Token X1".to_owned(),
                    amount: 1,
                    list_price: 10_000,
                }],
                ..StoreTable::default()
            },
            ..PlayerContent::default()
        });
        world
            .admit(&admit_at(PLAYER, 0.0, true), NOW, SERVER_TIME)
            .expect("admit");
        world.combat_messages.clear();
        let entity = world.player_by_number(PLAYER).expect("player");
        let header = FacetHeader::of(component::ENTITLEMENT, PLAYER);
        world.store_purchase(
            entity,
            header,
            &StorePurchase {
                language: b"en-US".to_vec(),
                currency: b"USD".to_vec(),
                country: b"US".to_vec(),
                word_a: 1,
                transaction: [7; RAW_ID_LEN],
                offer: TOKEN.as_bytes().to_vec(),
                offer_again: TOKEN.as_bytes().to_vec(),
                byte_b: 0,
                product: STORE_CURRENCY.as_bytes().to_vec(),
                price: 0,
                byte_c: 1,
            },
        );
        let grants = world.take_store_grants();
        let [grant] = &grants[..] else {
            panic!("one grant");
        };
        assert_eq!(
            (&grant.kind, grant.price),
            (&GrantKind::Token { amount: 1 }, 0),
            "a free store"
        );
        world.combat_messages.clear();
        let held = AccountConsumable {
            entitlement: TOKEN.to_owned(),
            amount: 15,
            purchases: 21,
            updated_at_ms: 0,
        };
        let wallet = Wallet::default();
        assert!(!world.store_answered(
            grant,
            GrantAnswer::Token(Ok(Purchase::Bought { held, wallet }))
        ));
        let answers: Vec<ServerMessage> = std::mem::take(&mut world.combat_messages)
            .into_iter()
            .filter(|out| out.connection == connection(PLAYER))
            .flat_map(|out| out.envelopes)
            .filter_map(|bytes| {
                let envelope = Envelope::decode(&bytes).ok()?;
                ServerMessage::decode(envelope.type_index()?, &envelope.body)?.ok()
            })
            .collect();
        let [ServerMessage::RequestSuccess(RequestSuccessMsg(done))] = &answers[..] else {
            panic!("1285: {answers:?}");
        };
        assert_eq!(done.header, header);
        let [token, wallet] = &done.entitlements[..] else {
            panic!("the token and the wallet");
        };
        assert_eq!(
            (token.kind, token.total, token.name.as_slice()),
            (CONSUMABLE_ENTITLEMENT_KIND, 15, &b"Barbershop Token"[..])
        );
        assert_eq!(wallet.kind, WALLET_ENTITLEMENT_KIND);
    }

    #[test]
    fn a_purchase_answers_as_a_retail_purchase() {
        let grant = StoreGrant {
            number: 1,
            account: Uuid::from_u128(1),
            entitlement: "Ent_Barbershop_Token".to_owned(),
            price: 0,
            kind: GrantKind::Token { amount: 1 },
            header: FacetHeader {
                target: 5,
                entity: 4,
            },
            request: [7; RAW_ID_LEN],
            transaction: b"1".to_vec(),
        };
        let ServerMessage::RequestInitialized(RequestInitializedMsg(started)) = grant.started()
        else {
            panic!("5936");
        };
        assert_eq!(started.status, RETAIL_REQUEST_STATUS);
        assert!(started.debits.is_empty() && started.done);
        let wallet = wallet_record(Wallet {
            marks: 0,
            updated_at_ms: 0,
        });
        assert_eq!((wallet.kind, wallet.total), (WALLET_ENTITLEMENT_KIND, 0));
        assert_eq!(wallet.alias, STORE_CURRENCY.as_bytes());
    }
}
