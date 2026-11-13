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
//! token is spent. Not sent: 3585 and 4147 (see [`super::barber`]).
//!
//! A refused request gets 3113 `OnRequestFail` ([`RequestFailMsg`]): the request's body, as its
//! 5936 has it, and a reason text. Observed (client handler `FUN_14301a3e0`): the client raises
//! the store event `OnEntitlementError` with both, its one answer to a failed request. A
//! purchase refused before it starts (a closed store, an offer that is not in the store table)
//! gets the 3113 of the request the purchase asked for, and a 5818 of an entitlement that is no
//! status effect token the 3113 of a use of it ([`SimWorld::token_debit`]). No capture holds a
//! 3113, so retail's
//! status and text are Unknown; the text of each reason is this server's own (Inferred).
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
    CONSUMABLE_ENTITLEMENT_KIND, ClaimEntitlementFailMsg, DEBIT_REQUEST_STATUS,
    DURABLE_ENTITLEMENT_KIND, EntitlementDebit, EntitlementRecord, EntitlementRequest,
    RETAIL_DEBIT_TAIL, RETAIL_REQUEST_STATUS, RequestFailMsg, RequestInitializedMsg,
    RequestSuccessMsg, StorePurchase, WALLET_ENTITLEMENT_KIND,
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
/// The 5108 code of a claim of an entitlement this server never grants. Inferred: the client
/// passes the code on as an integer (`OnClaimEntitlementFail`); its retail values are Unknown, and
/// 0 is left free for "no failure".
const CLAIM_NOT_GRANTED: u32 = 1;
/// The done flag of a purchase's answers (Observed: set on every retail purchase answer).
const PURCHASE_DONE: bool = true;
/// The 3113 reason of a purchase while the store is closed (Inferred).
const REASON_STORE_CLOSED: &str = "StoreClosed";
/// The 3113 reason of an offer that is not in the store table (Inferred).
const REASON_UNKNOWN_OFFER: &str = "UnknownOffer";
/// The 3113 reason of a wallet below the price (Inferred).
const REASON_TOO_FEW_MARKS: &str = "InsufficientFunds";
/// The 3113 reason of a use of a token the account does not hold (Inferred).
pub(super) const REASON_NO_TOKEN: &str = "NoToken";
/// The 3113 reason of a request the account store did not save (Inferred).
pub(super) const REASON_NOT_SAVED: &str = "ServiceError";
/// The 3113 reason of a use of an entitlement that is no status effect token (Inferred).
const REASON_UNKNOWN_TOKEN: &str = "UnknownToken";
/// The done flag of a status effect token use's answers (Observed: set on its 5936 and 1285,
/// `Playthrough_Part_02`).
const TOKEN_USE_DONE: bool = true;

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
        let answer = request_answer(
            self.header,
            self.request,
            self.transaction.clone(),
            entitlements,
            !barber,
        );
        if use_of_token {
            token_use(answer, &self.entitlement)
        } else {
            answer
        }
    }

    /// 3113: the request failed for `reason`, with the body of its 5936.
    pub(super) fn failed(&self, reason: &str) -> ServerMessage {
        request_failed(self.answer(Vec::new()), reason)
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

/// The answer to a purchase request on `header` with the request id `request` and the
/// transaction number `transaction`, listing `entitlements`: status 2 and no debit (Observed: every
/// retail purchase answer).
fn request_answer(
    header: FacetHeader,
    request: [u8; RAW_ID_LEN],
    transaction: Vec<u8>,
    entitlements: Vec<EntitlementRecord>,
    done: bool,
) -> EntitlementRequest {
    EntitlementRequest {
        header,
        request,
        transaction,
        status: RETAIL_REQUEST_STATUS,
        debits: Vec::new(),
        entitlements,
        done,
    }
}

/// `answer` as the answer to a use of one token of `entitlement`: status 4 and its debit
/// (Observed: every retail barber change and status effect token use).
fn token_use(mut answer: EntitlementRequest, entitlement: &str) -> EntitlementRequest {
    answer.status = DEBIT_REQUEST_STATUS;
    answer.debits = vec![EntitlementDebit {
        entitlement: entitlement.as_bytes().to_vec(),
        tail: RETAIL_DEBIT_TAIL,
    }];
    answer
}

/// 3113 of the failed request `request`, for `reason`.
fn request_failed(request: EntitlementRequest, reason: &str) -> ServerMessage {
    ServerMessage::RequestFail(RequestFailMsg {
        request,
        reason: reason.as_bytes().to_vec(),
    })
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
    /// grant for the loop, at its price. A closed store or another offer is refused with 3113.
    pub(super) fn store_purchase(
        &mut self,
        entity: Entity,
        header: FacetHeader,
        purchase: &StorePurchase,
    ) {
        let offer = String::from_utf8_lossy(&purchase.offer).into_owned();
        if !self.content.shop.enabled {
            tracing::info!(offer, "store purchase refused: the store is closed");
            self.refuse_purchase(entity, header, purchase.transaction, REASON_STORE_CLOSED);
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
            tracing::info!(offer, "store purchase refused: no such store offer");
            self.refuse_purchase(entity, header, purchase.transaction, REASON_UNKNOWN_OFFER);
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

    /// Player `entity`'s purchase on `header` with the request id `request` is refused before it
    /// starts: it gets 3113 for `reason` with a new transaction number.
    fn refuse_purchase(
        &mut self,
        entity: Entity,
        header: FacetHeader,
        request: [u8; RAW_ID_LEN],
        reason: &str,
    ) {
        count_call("store_purchase", false);
        let Some(connection) = self
            .world
            .get::<Player>(entity)
            .map(|player| player.connection.clone())
        else {
            return;
        };
        let transaction = self.next_transaction();
        let refused = request_answer(header, request, transaction, Vec::new(), PURCHASE_DONE);
        send_now(
            &mut self.combat_messages,
            &connection,
            &[request_failed(refused, reason)],
        );
    }

    /// The store request `grant` failed for `reason`: the player, if it is still on this sim,
    /// gets its 3113, then `then` when given.
    pub(super) fn store_request_failed(
        &mut self,
        grant: &StoreGrant,
        reason: &str,
        then: Option<ServerMessage>,
    ) {
        let Some(connection) = self.connection_of_number(grant.number) else {
            return;
        };
        let answers: Vec<ServerMessage> =
            std::iter::once(grant.failed(reason)).chain(then).collect();
        send_now(&mut self.combat_messages, &connection, &answers);
    }

    /// Player `entity` claims the entitlement `entitlement` (5107, the CRC-32 of its lowercase id)
    /// on `header`. No entitlement is granted on this server, so none can be claimed: the claim
    /// gets 5108 on its own header (Observed: retail's 5109 to a claim came on the claim's own
    /// header, `ExternalRewards` 1786896182018).
    pub(super) fn claim_entitlement(
        &mut self,
        entity: Entity,
        header: FacetHeader,
        entitlement: u32,
    ) {
        count_call("claim_entitlement", false);
        tracing::info!(
            entitlement,
            "entitlement claim refused: none is granted here"
        );
        let Some(connection) = self
            .world
            .get::<Player>(entity)
            .map(|player| player.connection.clone())
        else {
            return;
        };
        let failed = ServerMessage::ClaimEntitlementFail(ClaimEntitlementFailMsg {
            header,
            code: CLAIM_NOT_GRANTED,
        });
        send_now(&mut self.combat_messages, &connection, &[failed]);
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
    /// is refused before any request starts: it gets the 3113 of a use of that entitlement,
    /// with a new request id and transaction, and no 5936. Observed: 5818 holds only its header
    /// and the entitlement (no request id the client could wait on), and the client's 3113
    /// handler `FUN_14301a3e0` raises the store event `OnEntitlementError`, its one answer to a
    /// failed request; a purchase refused before it starts gets its 3113 alone too. No capture
    /// holds a refused 5818 (Inferred: the answer of the other refusals).
    ///
    /// [`EffectTokens`]: nw_content::store::EffectTokens
    pub(super) fn token_debit(&mut self, entity: Entity, entitlement: &[u8]) {
        let entitlement = String::from_utf8_lossy(entitlement).into_owned();
        // Observed: a new UUID v4 as the request id of each retail debit.
        let request_id = Uuid::new_v4().into_bytes();
        let Some(effect) = self
            .content
            .effect_tokens
            .effect_of(&entitlement)
            .map(str::to_owned)
        else {
            count_call("token_debit", false);
            tracing::info!(entitlement, "token use refused: no status effect token");
            self.refuse_token_use(entity, &entitlement, request_id);
            return;
        };
        let kind = GrantKind::StatusEffectToken { effect };
        self.start_store_request(entity, entitlement, NO_PRICE, kind, None, request_id);
    }

    /// Player `entity`'s use of a token of `entitlement` with the request id `request` is refused
    /// before it starts: it gets 3113 for [`REASON_UNKNOWN_TOKEN`] with a new transaction number.
    fn refuse_token_use(&mut self, entity: Entity, entitlement: &str, request: [u8; RAW_ID_LEN]) {
        let Some((number, connection)) = self
            .world
            .get::<Player>(entity)
            .map(|player| (player.number, player.connection.clone()))
        else {
            return;
        };
        let header = FacetHeader::of(component::ENTITLEMENT, number);
        let transaction = self.next_transaction();
        let refused = token_use(
            request_answer(header, request, transaction, Vec::new(), TOKEN_USE_DONE),
            entitlement,
        );
        send_now(
            &mut self.combat_messages,
            &connection,
            &[request_failed(refused, REASON_UNKNOWN_TOKEN)],
        );
    }

    /// The loop spent a status effect token for `grant` (`answer`): the player, if it is still
    /// on this sim, gets 1285 with the count left and holds the token's effect from now. An
    /// account without a token, or a failed use, counts as refused, logs and gets 3113.
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
                self.store_request_failed(grant, REASON_NO_TOKEN, None);
                return;
            }
            Err(error) => {
                count_call("token_debit", false);
                tracing::error!(%error, account = %grant.account, entitlement = grant.entitlement, "token not spent");
                self.store_request_failed(grant, REASON_NOT_SAVED, None);
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
    /// call counts as refused, logs, and the buyer, if it is still on this sim, gets 3113) for a
    /// wallet below the price or a failed save: the buyer holds nothing new.
    fn paid<H>(
        &mut self,
        grant: &StoreGrant,
        answer: Result<Purchase<H>, DbError>,
    ) -> Option<(H, Wallet)> {
        let reason = match answer {
            Ok(Purchase::Bought { held, wallet } | Purchase::AlreadyHeld { held, wallet }) => {
                count_call("store_purchase", true);
                return Some((held, wallet));
            }
            Ok(Purchase::TooPoor { wallet }) => {
                count_call("store_purchase", false);
                tracing::info!(
                    entitlement = grant.entitlement,
                    price = grant.price,
                    marks = wallet.marks,
                    "store purchase refused: the wallet holds too few Marks of Fortune"
                );
                REASON_TOO_FEW_MARKS
            }
            Err(error) => {
                count_call("store_purchase", false);
                tracing::error!(%error, entitlement = grant.entitlement, account = %grant.account, "store purchase not saved");
                REASON_NOT_SAVED
            }
        };
        self.store_request_failed(grant, reason, None);
        None
    }

    /// The loop ran the mount purchase `grant` (`answer`): when the account holds the mount
    /// now, the buyer, if it is still on this sim, may use it and gets 1285 with it and the
    /// wallet.
    pub fn store_granted(&mut self, grant: &StoreGrant, answer: Result<Purchase, DbError>) {
        let Some((held, wallet)) = self.paid(grant, answer) else {
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
        let Some((held, wallet)) = self.paid(grant, answer) else {
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
    fn a_refused_purchase_gets_its_request_back_in_3113() {
        use std::sync::Arc;

        use nw_login::config::ShopConfig;
        use nw_reflect::Envelope;

        use crate::player_content::PlayerContent;
        use crate::world::tests::{NOW, SERVER_TIME, admit_at, connection, world_with_spawns};

        const PLAYER: u64 = 7;
        const REQUEST: [u8; RAW_ID_LEN] = [9; RAW_ID_LEN];
        let failures = |world: &mut SimWorld| -> Vec<RequestFailMsg> {
            std::mem::take(&mut world.combat_messages)
                .into_iter()
                .filter(|out| out.connection == connection(PLAYER))
                .flat_map(|out| out.envelopes)
                .filter_map(|bytes| {
                    let envelope = Envelope::decode(&bytes).ok()?;
                    match ServerMessage::decode(envelope.type_index()?, &envelope.body)?.ok()? {
                        ServerMessage::RequestFail(failed) => Some(failed),
                        _ => None,
                    }
                })
                .collect()
        };
        let mut world = world_with_spawns(Vec::new());
        world
            .admit(&admit_at(PLAYER, 0.0, true), NOW, SERVER_TIME)
            .expect("admit");
        world.combat_messages.clear();
        let entity = world.player_by_number(PLAYER).expect("player");
        let header = FacetHeader::of(component::ENTITLEMENT, PLAYER);
        let purchase = StorePurchase {
            language: b"en-US".to_vec(),
            currency: b"USD".to_vec(),
            country: b"US".to_vec(),
            word_a: 1,
            transaction: REQUEST,
            offer: b"Ent_Not_In_The_Table".to_vec(),
            offer_again: b"Ent_Not_In_The_Table".to_vec(),
            byte_b: 0,
            product: STORE_CURRENCY.as_bytes().to_vec(),
            price: 0,
            byte_c: 1,
        };
        world.store_purchase(entity, header, &purchase);
        assert!(world.take_store_grants().is_empty(), "nothing is bought");
        let [unknown] = &failures(&mut world)[..] else {
            panic!("one 3113");
        };
        assert_eq!(unknown.reason, REASON_UNKNOWN_OFFER.as_bytes());
        assert_eq!(
            (unknown.request.header, unknown.request.request),
            (header, REQUEST)
        );
        assert_eq!(unknown.request.status, RETAIL_REQUEST_STATUS);
        assert!(unknown.request.debits.is_empty() && unknown.request.entitlements.is_empty());

        world.content = Arc::new(PlayerContent {
            shop: ShopConfig {
                enabled: false,
                ..ShopConfig::default()
            },
            ..PlayerContent::default()
        });
        world.store_purchase(entity, header, &purchase);
        let [closed] = &failures(&mut world)[..] else {
            panic!("one 3113");
        };
        assert_eq!(closed.reason, REASON_STORE_CLOSED.as_bytes());
        assert!(
            closed.request.transaction > unknown.request.transaction,
            "a new transaction number"
        );

        let grant = StoreGrant {
            number: PLAYER,
            account: Uuid::from_u128(1),
            entitlement: "Ent_Mount_MTX_7_bear_Ronin".to_owned(),
            price: 20_000,
            kind: GrantKind::Mount,
            header,
            request: REQUEST,
            transaction: b"1".to_vec(),
        };
        let wallet = Wallet::default();
        world.store_answered(&grant, GrantAnswer::Mount(Ok(Purchase::TooPoor { wallet })));
        let [poor] = &failures(&mut world)[..] else {
            panic!("one 3113");
        };
        assert_eq!(poor.reason, REASON_TOO_FEW_MARKS.as_bytes());
        let ServerMessage::RequestInitialized(RequestInitializedMsg(started)) = grant.started()
        else {
            panic!("5936");
        };
        assert_eq!(poor.request, started, "the body of the request's 5936");
    }

    /// A 5818 for an entitlement that is no status effect token starts no request: it gets only
    /// the 3113 of a use of that entitlement, on the player's entitlement facet.
    #[test]
    fn an_unknown_token_use_gets_3113_alone() {
        use nw_reflect::Envelope;

        use crate::world::tests::{NOW, SERVER_TIME, admit_at, connection, world_with_spawns};

        const PLAYER: u64 = 7;
        const UNKNOWN: &str = "Ent_StatusEffect_Token_Not_In_The_Data";
        let mut world = world_with_spawns(Vec::new());
        world
            .admit(&admit_at(PLAYER, 0.0, true), NOW, SERVER_TIME)
            .expect("admit");
        world.combat_messages.clear();
        let entity = world.player_by_number(PLAYER).expect("player");
        world.token_debit(entity, UNKNOWN.as_bytes());
        assert!(world.take_store_grants().is_empty(), "no token is spent");
        let answers: Vec<ServerMessage> = std::mem::take(&mut world.combat_messages)
            .into_iter()
            .filter(|out| out.connection == connection(PLAYER))
            .flat_map(|out| out.envelopes)
            .filter_map(|bytes| {
                let envelope = Envelope::decode(&bytes).ok()?;
                ServerMessage::decode(envelope.type_index()?, &envelope.body)?.ok()
            })
            .collect();
        let [ServerMessage::RequestFail(refused)] = &answers[..] else {
            panic!("one 3113 and no 5936: {answers:?}");
        };
        assert_eq!(refused.reason, REASON_UNKNOWN_TOKEN.as_bytes());
        let request = &refused.request;
        assert_eq!(
            request.header,
            FacetHeader::of(component::ENTITLEMENT, PLAYER)
        );
        assert_eq!(request.status, DEBIT_REQUEST_STATUS);
        assert_eq!(
            request.debits,
            [EntitlementDebit {
                entitlement: UNKNOWN.as_bytes().to_vec(),
                tail: RETAIL_DEBIT_TAIL,
            }]
        );
        assert!(request.entitlements.is_empty() && request.done);
        assert_ne!(request.request, [0; RAW_ID_LEN], "a new request id");
    }

    #[test]
    fn a_claim_gets_5108_on_its_own_header() {
        use nw_reflect::Envelope;

        use crate::world::tests::{NOW, SERVER_TIME, admit_at, connection, world_with_spawns};

        const PLAYER: u64 = 7;
        /// The entitlement of the retail claim (`ExternalRewards` 1786896181850).
        const ENTITLEMENT: u32 = 0xf938_e156;
        let mut world = world_with_spawns(Vec::new());
        world
            .admit(&admit_at(PLAYER, 0.0, true), NOW, SERVER_TIME)
            .expect("admit");
        world.combat_messages.clear();
        let entity = world.player_by_number(PLAYER).expect("player");
        let header = FacetHeader::of(component::ENTITLEMENT, PLAYER);
        world.claim_entitlement(entity, header, ENTITLEMENT);
        let answers: Vec<ServerMessage> = std::mem::take(&mut world.combat_messages)
            .into_iter()
            .filter(|out| out.connection == connection(PLAYER))
            .flat_map(|out| out.envelopes)
            .filter_map(|bytes| {
                let envelope = Envelope::decode(&bytes).ok()?;
                ServerMessage::decode(envelope.type_index()?, &envelope.body)?.ok()
            })
            .collect();
        assert_eq!(
            answers,
            [ServerMessage::ClaimEntitlementFail(
                ClaimEntitlementFailMsg {
                    header,
                    code: CLAIM_NOT_GRANTED,
                }
            )]
        );
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
