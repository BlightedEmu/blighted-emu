// SPDX-License-Identifier: AGPL-3.0-or-later
//! Size limits for `LoginQueueRequest` fields. The type is in `nw-protocol`.

/// Longest id string this server accepts, in bytes.
pub(crate) const MAX_ID_LEN: usize = 128;

/// Longest Steam auth ticket this server stores, in bytes.
pub(crate) const MAX_STEAM_TICKET_LEN: usize = 4096;

/// Most capability strings accepted on one request.
pub(crate) const MAX_CAPABILITY_COUNT: usize = 64;

/// Longest one capability string, in bytes.
pub(crate) const MAX_CAPABILITY_LEN: usize = 128;
