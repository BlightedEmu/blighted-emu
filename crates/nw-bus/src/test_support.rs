// SPDX-License-Identifier: AGPL-3.0-or-later
//! Helpers for tests on the Docker Compose NATS (feature `test-util`).

use crate::client::Bus;

/// Environment variable with the NATS URL of infrastructure tests.
pub const TEST_NATS_URL_ENV: &str = "NW_TEST_NATS_URL";
/// The Docker Compose NATS.
pub const DEFAULT_TEST_NATS_URL: &str = "nats://127.0.0.1:4222";

/// NATS URL for infrastructure tests (`NW_TEST_NATS_URL` or the Docker Compose default).
pub fn test_nats_url() -> String {
    std::env::var(TEST_NATS_URL_ENV).unwrap_or_else(|_| DEFAULT_TEST_NATS_URL.to_owned())
}

/// A bus on [`test_nats_url`].
///
/// # Panics
///
/// When NATS cannot be reached.
pub async fn test_bus() -> Bus {
    Bus::connect(&test_nats_url())
        .await
        .expect("connect to the test NATS")
}
