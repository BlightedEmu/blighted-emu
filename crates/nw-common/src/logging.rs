// SPDX-License-Identifier: AGPL-3.0-or-later
//! Sets up structured logging for a program.
//!
//! The filter uses the `tracing` directive syntax, for example `info,nw_sim=debug`.

use tracing_subscriber::EnvFilter;
use tracing_subscriber::filter::ParseError;

/// Error returned by [`parse_log_filter`] and [`init_logging`].
#[derive(Debug, thiserror::Error)]
pub enum LoggingError {
    /// The filter text is not valid `tracing` directive syntax.
    #[error("invalid log filter `{filter}`: {source}")]
    InvalidFilter {
        /// The filter text that failed to parse.
        filter: String,
        /// The parser error.
        #[source]
        source: ParseError,
    },
    /// A global logger is already installed in this process.
    #[error("cannot install the global logger: {0}")]
    GlobalInstall(String),
}

/// Parses a log filter such as `info,nw_sim=debug`.
pub fn parse_log_filter(filter: &str) -> Result<EnvFilter, LoggingError> {
    EnvFilter::try_new(filter).map_err(|source| LoggingError::InvalidFilter {
        filter: filter.to_owned(),
        source,
    })
}

/// Installs the global logger with the given filter. Call it once at program start.
pub fn init_logging(filter: &str) -> Result<(), LoggingError> {
    let env_filter = parse_log_filter(filter)?;
    tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .try_init()
        .map_err(|error| LoggingError::GlobalInstall(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID_FILTER: &str = "info,nw_common=debug";
    const INVALID_LEVEL_FILTER: &str = "nw_common=loud";

    #[test]
    fn valid_filter_parses() {
        assert!(parse_log_filter(VALID_FILTER).is_ok());
    }

    #[test]
    fn unknown_level_is_rejected_with_the_filter_text() {
        let error = parse_log_filter(INVALID_LEVEL_FILTER).expect_err("loud is not a level");
        assert!(
            matches!(&error, LoggingError::InvalidFilter { filter, .. } if filter == INVALID_LEVEL_FILTER),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn init_rejects_invalid_filter_before_installing() {
        let error = init_logging(INVALID_LEVEL_FILTER).expect_err("invalid filter");
        assert!(
            matches!(error, LoggingError::InvalidFilter { .. }),
            "unexpected error: {error}"
        );
    }

    // The only test in this binary that installs the global logger.
    #[test]
    fn second_install_is_an_error() {
        init_logging(VALID_FILTER).expect("first install succeeds");
        let error = init_logging(VALID_FILTER).expect_err("second install fails");
        assert!(
            matches!(error, LoggingError::GlobalInstall(_)),
            "unexpected error: {error}"
        );
    }
}
