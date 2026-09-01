// SPDX-License-Identifier: AGPL-3.0-or-later
//! Length limits for account and character text.
//!
//! The client protocol note does not record these limits.
//! They are the limits this server stores.

/// Longest username, in bytes.
pub const MAX_USERNAME_LEN: usize = 64;

/// Shortest password, in bytes.
pub const MIN_PASSWORD_LEN: usize = 8;

/// Longest password, in bytes.
pub const MAX_PASSWORD_LEN: usize = 128;

/// Longest character name, in bytes.
pub const MAX_CHARACTER_NAME_LEN: usize = 64;

/// Longest world id, in bytes.
pub const MAX_WORLD_ID_LEN: usize = 64;

/// Argon2id memory cost in kibibytes. This matches the Argon2 recommended default.
pub const ARGON2_MEMORY_KIB: u32 = 19 * 1024;

/// Argon2id passes.
pub const ARGON2_ITERATIONS: u32 = 2;

/// Argon2id lanes.
pub const ARGON2_PARALLELISM: u32 = 1;

/// Salt length in bytes.
pub const PASSWORD_SALT_LEN: usize = 16;
