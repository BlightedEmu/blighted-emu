// SPDX-License-Identifier: AGPL-3.0-or-later
//! The name and crest of the companies that the game modes name in an ownership: the carrier's
//! company on a carried flag ([`super::capture_the_flag`]), the user's company on an Outpost Rush
//! vat, its barrel and its area ([`super::siege_areas`]), and a war side's company on the fort's
//! structures ([`super::siege_structures`]).
//!
//! Observed: the retail ownerships name the company with its name and crest also when it holds
//! no claim (`OPR_TEST3` 1714.326 s: the vat user's company; `CTF04` 205.526 s: the carrier's).
//! The sim knows the name and crest of a company that holds a claim from the claims
//! ([`super::land_claims::Territories::company`]); every other company's comes from the world
//! database (`nw_db::company_listings`): a game mode asks for the companies of its players
//! ([`SimWorld::ask_company_looks`]), the server reads them in one task
//! ([`SimWorld::take_company_reads`]) and gives the answer back ([`SimWorld::companies_read`]).
//! Inferred: each company is read once per sim run, and an ownership made before its answer
//! names no company (the war's structures, which spawn before it, show it when it comes:
//! [`SimWorld::show_side_looks`]); a company the database does not hold, or whose read failed,
//! is not asked again.

use std::collections::HashMap;

use bevy_ecs::entity::Entity;
use nw_messages::buildables::{GUILD_ID_LEN, GuildCrest};
use nw_messages::social::crest_of_bytes;
use uuid::Uuid;

use super::{Player, SimWorld};

/// The name and crest of a company as an ownership names it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CompanyLook {
    pub(super) name: Vec<u8>,
    pub(super) crest: GuildCrest,
}

/// The looks read from the world database (module doc).
#[derive(Debug, Default)]
pub(crate) struct CompanyLooks {
    /// By company id; `None` while its read runs or after a read that did not hold it.
    looks: HashMap<Uuid, Option<CompanyLook>>,
    /// The companies to read next.
    reads: Vec<Uuid>,
}

impl SimWorld {
    /// Asks for the look of the company of each of `players` that has one and was not asked for
    /// yet (module doc).
    pub(super) fn ask_company_looks(&mut self, players: &[Entity]) {
        let ids: Vec<Uuid> = players
            .iter()
            .filter_map(|entity| self.world.get::<Player>(*entity))
            .filter_map(|player| player.social.company())
            .map(Uuid::from_bytes)
            .collect();
        self.ask_company_ids(ids);
    }

    /// Asks for the looks of companies `ids` not asked for yet; the zero id of no company is
    /// never asked.
    pub(super) fn ask_company_ids(&mut self, ids: impl IntoIterator<Item = Uuid>) {
        let looks = &mut self.company_looks;
        for id in ids {
            if !id.is_nil() && !looks.looks.contains_key(&id) {
                looks.looks.insert(id, None);
                looks.reads.push(id);
            }
        }
    }

    /// The look of company `id`: from its claims, else from the world database's answer;
    /// `None` while neither knows it.
    pub(super) fn company_look(&self, id: [u8; GUILD_ID_LEN]) -> Option<CompanyLook> {
        if let Some(owner) = self.territories.company(id) {
            return Some(CompanyLook {
                name: owner.guild_name.clone(),
                crest: owner.crest,
            });
        }
        self.company_looks
            .looks
            .get(&Uuid::from_bytes(id))?
            .clone()
    }

    /// The company of player `user` and its look ([`Self::company_look`]); `None` for a player
    /// with no company or whose company's look is not known yet.
    pub(super) fn company_look_of(
        &self,
        user: Entity,
    ) -> Option<([u8; GUILD_ID_LEN], CompanyLook)> {
        let id = self.world.get::<Player>(user)?.social.company()?;
        Some((id, self.company_look(id)?))
    }

    /// The companies the game modes ask the world database for (the server reads them).
    pub(crate) fn take_company_reads(&mut self) -> Vec<Uuid> {
        std::mem::take(&mut self.company_looks.reads)
    }

    /// The world database's answer to [`Self::take_company_reads`]: each company's name and
    /// crest, for the next ownership that names it.
    pub(crate) fn companies_read(&mut self, listings: &[nw_db::CompanyListing]) {
        let read: Vec<Uuid> = listings.iter().map(|listing| listing.id).collect();
        for listing in listings {
            self.company_looks.looks.insert(
                listing.id,
                Some(CompanyLook {
                    name: listing.name.clone().into_bytes(),
                    crest: crest_of_bytes(&listing.crest),
                }),
            );
        }
        self.show_side_looks(&read);
    }
}

#[cfg(test)]
pub(in crate::world) mod tests {
    use nw_bus::{OwnStateDelivery, SocialDeliveries, SocialStateKind};
    use nw_messages::social::states::GuildsState;
    use nw_messages::wire::Writer;

    use super::*;

    /// The faction of a made-up company listing.
    const LISTED_FACTION: u8 = 1;
    /// The member count of a made-up company listing.
    const LISTED_MEMBERS: u32 = 1;

    /// Player `entity` joins company `company` (its own guilds state, as the social service
    /// sends it).
    pub(in crate::world) fn join_company(world: &mut SimWorld, entity: Entity, company: Uuid) {
        let character = world.world.get::<Player>(entity).expect("player").character;
        let mut writer = Writer::new();
        GuildsState {
            guild_id: Some(*company.as_bytes()),
            ..GuildsState::default()
        }
        .write(&mut writer)
        .expect("writes");
        world.apply_social_states(&SocialDeliveries {
            own_states: vec![OwnStateDelivery {
                character,
                kind: SocialStateKind::Guilds,
                body: writer.finish(),
            }],
            ..SocialDeliveries::default()
        });
    }

    /// The world database's listing of made-up company `id` named `name` with no claim.
    pub(in crate::world) fn listing(id: Uuid, name: &str) -> nw_db::CompanyListing {
        nw_db::CompanyListing {
            id,
            name: name.to_owned(),
            governor: None,
            faction: LISTED_FACTION,
            crest: [0; nw_db::CREST_LEN],
            members: LISTED_MEMBERS,
            claims: 0,
            siege_hour: 0.0,
        }
    }

    /// A company is asked for once, its look comes from the answer, and the zero id of no
    /// company is never asked.
    #[test]
    fn a_company_is_read_once_and_its_look_comes_from_the_answer() {
        const COMPANY: Uuid = Uuid::from_u128(0xc0_ffee);
        const NAME: &str = "Test Company";
        let mut world = crate::world::tests::world();
        world.ask_company_ids([COMPANY, Uuid::nil()]);
        assert_eq!(world.take_company_reads(), vec![COMPANY]);
        world.ask_company_ids([COMPANY]);
        assert!(world.take_company_reads().is_empty(), "asked once");
        assert_eq!(world.company_look(*COMPANY.as_bytes()), None, "not read yet");
        world.companies_read(&[listing(COMPANY, NAME)]);
        assert_eq!(
            world.company_look(*COMPANY.as_bytes()),
            Some(CompanyLook {
                name: NAME.as_bytes().to_vec(),
                crest: crest_of_bytes(&[0; nw_db::CREST_LEN]),
            })
        );
    }
}
