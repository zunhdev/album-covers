//! The error type. Every message is written for the person running `covers`.

use std::io;
use std::path::PathBuf;
use std::time::Duration;

use crate::catalog::Service;

/// Result alias for this crate.
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Everything that can go wrong after the command line was accepted.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// No usable client ID and/or secret was found.
    #[error(
        "missing {service} credentials: {missing}\n\n\
         Create an app at {dashboard} to get them, then put them in\n  {config_file}\n\
         (or in .env in the current directory, or in environment variables). For example:\n\n  \
         {prefix}_CLIENT_ID=your-client-id\n  {prefix}_CLIENT_SECRET=your-client-secret",
        missing = .missing.join(", "),
        dashboard = .service.developer_dashboard(),
        config_file = .config_file.as_ref().map_or_else(|| "the config file".into(), |path| path.display().to_string()),
        prefix = .service.env_prefix(),
    )]
    MissingCredentials {
        service: Service,
        missing: Vec<String>,
        /// The per-user credentials file, if a home directory is known.
        config_file: Option<PathBuf>,
    },

    /// A credentials file exists but can't be read or parsed.
    #[error("can't read {}: {reason}", .path.display())]
    CredentialsFile { path: PathBuf, reason: String },

    /// The service rejected the client ID or secret.
    #[error(
        "{service} did not accept the client credentials: {detail}\n\
         Check {prefix}_CLIENT_ID and {prefix}_CLIENT_SECRET.",
        prefix = .service.env_prefix(),
    )]
    InvalidCredentials { service: Service, detail: String },

    /// The artist doesn't exist in the requested country's catalog.
    #[error("{service} has no artist with ID {id} (catalog country: {country})")]
    ArtistNotFound {
        service: Service,
        id: String,
        country: String,
    },

    /// HTTP 429 that did not clear in time.
    #[error(
        "{service} rate limit reached while {context}; try again {}",
        .retry_after.map_or_else(|| "later".into(), |wait| format!("in {}s", wait.as_secs())),
    )]
    RateLimited {
        service: Service,
        context: &'static str,
        retry_after: Option<Duration>,
    },

    /// Any other unsuccessful HTTP status.
    #[error(
        "{service} answered HTTP {status} while {context}{}",
        if .detail.is_empty() { String::new() } else { format!(": {}", .detail) },
    )]
    Status {
        service: Service,
        status: u16,
        context: &'static str,
        detail: String,
    },

    /// No response at all (DNS, connection, TLS, timeout…).
    #[error("network error while {context}: {source}")]
    Network {
        context: &'static str,
        #[source]
        source: Box<ureq::Error>,
    },

    /// A response that isn't what the API documents.
    #[error("unexpected response from {service} while {context}: {reason}")]
    UnexpectedResponse {
        service: Service,
        context: &'static str,
        reason: String,
    },

    /// Writing the output failed (for example, a closed pipe).
    #[error(transparent)]
    Output(#[from] io::Error),
}

impl Error {
    /// True for a closed output pipe (`covers … | head`), which isn't worth reporting.
    #[must_use]
    pub fn is_broken_pipe(&self) -> bool {
        matches!(self, Self::Output(err) if err.kind() == io::ErrorKind::BrokenPipe)
    }
}
