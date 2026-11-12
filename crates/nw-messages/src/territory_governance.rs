// SPDX-License-Identifier: AGPL-3.0-or-later
//! The town data of `MB::TerritoryInterfaceComponentReplicatedState` (type 1746): slot 1
//! `territoryProgressionData` and slot 2 `territoryGovernanceData` (child names and wire forms of
//! the client's constructor, `member_layouts` table). Numbers are big-endian; each list is a
//! prefix-coded count and its entries.
//!
//! **Observed** (territory upkeep study 2026-11-11, every 1746 body of the capture index that
//! sets slot 1 or 2: 72 bodies of 8 ledgers): retail sends them on the town's governor's desk and
//! town boards (a level entity: a card with a `GdeRef` only, the interact state at component 2 and
//! the 1746 at component 3), after the player opened it (`pay-territory-tax` 1783335279064, after
//! the desk interaction 3743 of kind 54 `@ui_governorsdesk` and the 2722 at 1783335279058), and on
//! the stations of a town (`fort-equipements-territory`). The governance data:
//!
//! - three Unix ns times: the upkeep due time ([`TerritoryGovernanceData::upkeep_due_ns`];
//!   Inferred: the one upkeep payment 2738 at 1783335287110 moved it by 604,800 s, and the
//!   current earnings interval of the unpaid week ended at it), a time 0 in every body (meaning
//!   Unknown), and a time 5 days before the due time in every body
//!   ([`TerritoryGovernanceData::upkeep_payable_ns`]; Inferred: from when the next payment is
//!   taken; the payment came between it and the due time, and moved it by 7 days too);
//! - per tax or fee type (`TaxOrFeeType` values 1 to 4) one Unix ns time (the same time for all
//!   four in every body; Inferred: when its rate was last set);
//! - the current earnings interval and the 4 before it: a total, per tax or fee type three
//!   numbers, and the interval's end (Unix ns; the past ends 604,800 s apart). Observed: the
//!   total is the sum of the first numbers (14,760,032 = 11,140,274 + 361,605 + 2,888,414 +
//!   369,739 in `pay-territory-tax`), so the first is the coin the type earned; the second and
//!   third are larger (meaning Unknown: Inferred the taxed amount and the count of taxed acts);
//! - a `u32`: the claim's territory level (Observed: 31, 35 and 27 for the desks of territories
//!   6, 2 and 4, their levels in the 3086 of the same ledgers).
//!
//! The progression data (slot 1, meanings Unknown beyond the forms): a list of (`u8`, (`u32`,
//! Unix ns, `u32`)) entries (5 per body, first bytes 1, 2, 3, 6, 7; the `u32` is the CRC-32 of a
//! town project id or 0), a list of (`u32`, `u64`) entries (project id CRCs with 0), one
//! (`u32`, `u64`, `u32`) entry (zero in every body), and a list of (`u32`, Unix ns) entries.

use crate::wire::{MessageError, Reader, Writer};

/// The most entries a list of this data may hold (a count beyond it is a misread body; the
/// longest retail list has 38).
const MAX_ENTRIES: u64 = 1_000;

/// Reads a list count, at most [`MAX_ENTRIES`].
fn count(reader: &mut Reader<'_>) -> Result<usize, MessageError> {
    let count = reader.prefixed()?;
    if count > MAX_ENTRIES {
        return Err(MessageError::CountTooLarge {
            count,
            limit: MAX_ENTRIES,
        });
    }
    // At most MAX_ENTRIES.
    Ok(count as usize)
}

/// Reads a list of `read` entries.
fn read_list<T>(
    reader: &mut Reader<'_>,
    mut read: impl FnMut(&mut Reader<'_>) -> Result<T, MessageError>,
) -> Result<Vec<T>, MessageError> {
    let entries = count(reader)?;
    (0..entries).map(|_| read(reader)).collect()
}

/// Writes a list of `entries` with `write`.
fn write_list<T>(
    writer: &mut Writer,
    entries: &[T],
    mut write: impl FnMut(&T, &mut Writer) -> Result<(), MessageError>,
) -> Result<(), MessageError> {
    // A `usize` count fits 64 bits.
    writer.prefixed(entries.len() as u64)?;
    entries.iter().try_for_each(|entry| write(entry, writer))
}

/// One (`u32`, Unix ns, `u32`) part of the progression data (meaning Unknown).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ProjectEntry {
    /// The CRC-32 of a lowercase town project id, or 0.
    pub project: u32,
    /// A Unix ns time, or 0.
    pub time_ns: u64,
    /// A number (0 to 1,700 in retail; meaning Unknown).
    pub value: u32,
}

impl ProjectEntry {
    fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        Ok(Self {
            project: reader.u32()?,
            time_ns: reader.u64()?,
            value: reader.u32()?,
        })
    }

    fn write(&self, writer: &mut Writer) {
        writer.u32(self.project).u64(self.time_ns).u32(self.value);
    }
}

/// One (`u8`, [`ProjectEntry`]) entry of the progression data's first list.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ProjectSlot {
    /// The first byte (1, 2, 3, 6 or 7 in retail; meaning Unknown).
    pub slot: u8,
    /// The entry.
    pub entry: ProjectEntry,
}

/// One (`u32`, `u64`) entry of the progression data's lists.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ProjectTime {
    /// The CRC-32 of a lowercase town project id.
    pub project: u32,
    /// A Unix ns time, or 0.
    pub time_ns: u64,
}

impl ProjectTime {
    fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        Ok(Self {
            project: reader.u32()?,
            time_ns: reader.u64()?,
        })
    }

    fn write(&self, writer: &mut Writer) -> Result<(), MessageError> {
        writer.u32(self.project).u64(self.time_ns);
        Ok(())
    }
}

/// `territoryProgressionData` (slot 1 of type 1746).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TerritoryProgressionData {
    /// The first list.
    pub slots: Vec<ProjectSlot>,
    /// The second list.
    pub projects: Vec<ProjectTime>,
    /// The single entry.
    pub entry: ProjectEntry,
    /// The last list.
    pub times: Vec<ProjectTime>,
}

impl TerritoryProgressionData {
    pub(crate) fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        Ok(Self {
            slots: read_list(reader, |reader| {
                Ok(ProjectSlot {
                    slot: reader.u8()?,
                    entry: ProjectEntry::read(reader)?,
                })
            })?,
            projects: read_list(reader, ProjectTime::read)?,
            entry: ProjectEntry::read(reader)?,
            times: read_list(reader, ProjectTime::read)?,
        })
    }

    pub(crate) fn write(&self, writer: &mut Writer) -> Result<(), MessageError> {
        write_list(writer, &self.slots, |slot, writer| {
            writer.u8(slot.slot);
            slot.entry.write(writer);
            Ok(())
        })?;
        write_list(writer, &self.projects, ProjectTime::write)?;
        self.entry.write(writer);
        write_list(writer, &self.times, ProjectTime::write)
    }
}

/// `TaxOrFeeType` values of the client (Inferred: the order of its strings `PropertyTax`,
/// `TradingTax`, `CraftingFee`, `RefiningFee`, from 1; retail lists hold exactly 1 to 4).
pub const TAX_TYPES: [u8; 4] = [1, 2, 3, 4];

/// When one tax or fee rate was last set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TaxChange {
    /// The `TaxOrFeeType`.
    pub tax: u8,
    /// Unix ns.
    pub changed_ns: u64,
}

/// What one tax or fee type brought in one earnings interval.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TaxEarnings {
    /// The `TaxOrFeeType`.
    pub tax: u8,
    /// The coin it earned, hundredths of a coin.
    pub earned: u64,
    /// The second number (Inferred: the taxed amount).
    pub taxed: u64,
    /// The third number (Inferred: the count of taxed acts).
    pub count: u64,
}

impl TaxEarnings {
    /// A type that earned nothing.
    pub const fn none(tax: u8) -> Self {
        Self {
            tax,
            earned: 0,
            taxed: 0,
            count: 0,
        }
    }
}

/// One earnings interval of a territory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EarningsInterval {
    /// The coin of every type (Observed: the sum of their `earned`).
    pub total: u64,
    /// Per tax or fee type.
    pub taxes: Vec<TaxEarnings>,
    /// When the interval ends, Unix ns.
    pub end_ns: u64,
}

impl EarningsInterval {
    fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        Ok(Self {
            total: reader.u64()?,
            taxes: read_list(reader, |reader| {
                Ok(TaxEarnings {
                    tax: reader.u8()?,
                    earned: reader.u64()?,
                    taxed: reader.u64()?,
                    count: reader.u64()?,
                })
            })?,
            end_ns: reader.u64()?,
        })
    }

    fn write(&self, writer: &mut Writer) -> Result<(), MessageError> {
        writer.u64(self.total);
        write_list(writer, &self.taxes, |tax, writer| {
            writer
                .u8(tax.tax)
                .u64(tax.earned)
                .u64(tax.taxed)
                .u64(tax.count);
            Ok(())
        })?;
        writer.u64(self.end_ns);
        Ok(())
    }
}

/// `territoryGovernanceData` (slot 2 of type 1746).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerritoryGovernanceData {
    /// When the upkeep is due, Unix ns (Inferred).
    pub upkeep_due_ns: u64,
    /// The second time (0 in every retail body; meaning Unknown).
    pub second_time_ns: u64,
    /// From when the next upkeep payment is taken, Unix ns (Inferred; 5 days before the due time
    /// in every retail body).
    pub upkeep_payable_ns: u64,
    /// When each tax or fee rate was last set.
    pub tax_changes: Vec<TaxChange>,
    /// The current earnings interval.
    pub current: EarningsInterval,
    /// The earnings intervals before it, newest last (Observed: in time order).
    pub history: Vec<EarningsInterval>,
    /// The claim's territory level.
    pub level: u32,
}

impl TerritoryGovernanceData {
    pub(crate) fn read(reader: &mut Reader<'_>) -> Result<Self, MessageError> {
        Ok(Self {
            upkeep_due_ns: reader.u64()?,
            second_time_ns: reader.u64()?,
            upkeep_payable_ns: reader.u64()?,
            tax_changes: read_list(reader, |reader| {
                Ok(TaxChange {
                    tax: reader.u8()?,
                    changed_ns: reader.u64()?,
                })
            })?,
            current: EarningsInterval::read(reader)?,
            history: read_list(reader, EarningsInterval::read)?,
            level: reader.u32()?,
        })
    }

    pub(crate) fn write(&self, writer: &mut Writer) -> Result<(), MessageError> {
        writer
            .u64(self.upkeep_due_ns)
            .u64(self.second_time_ns)
            .u64(self.upkeep_payable_ns);
        write_list(writer, &self.tax_changes, |change, writer| {
            writer.u8(change.tax).u64(change.changed_ns);
            Ok(())
        })?;
        self.current.write(writer)?;
        write_list(writer, &self.history, EarningsInterval::write)?;
        writer.u32(self.level);
        Ok(())
    }
}
