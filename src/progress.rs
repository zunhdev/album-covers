//! What the user sees on stderr while releases are being fetched.

use std::cell::Cell;
use std::fmt;
use std::time::Duration;

use indicatif::{ProgressBar, ProgressStyle};

use crate::catalog::Service;
use crate::http::{RetryObserver, Wait};

/// Receives progress notifications. `()` ignores them all.
pub trait Progress: RetryObserver {
    /// Fetching from `service` has started.
    fn start(&self, _service: Service) {}
    /// Fetching is over, successfully or not; remove any transient output.
    fn finish(&self) {}
}

impl Progress for () {}

/// A spinner on stderr, when stderr is a terminal:
///
/// ```text
/// ⠋ Fetching releases from TIDAL…
/// ⠙ Fetching releases from TIDAL… rate limited, retrying in 4s
/// ```
///
/// Otherwise nothing is drawn, and retry waits are printed as plain lines,
/// so logs still explain the pauses.
pub struct Spinner {
    bar: ProgressBar,
    service: Cell<Option<Service>>,
}

impl Spinner {
    #[must_use]
    pub fn new() -> Self {
        let bar = ProgressBar::new_spinner();
        if let Ok(style) = ProgressStyle::with_template("{spinner:.dim} {msg:.dim}") {
            bar.set_style(style);
        }
        Self {
            bar,
            service: Cell::new(None),
        }
    }

    fn base_message(&self) -> String {
        let service = self.service.get().map_or("the service", Service::name);
        format!("Fetching releases from {service}…")
    }
}

impl Default for Spinner {
    fn default() -> Self {
        Self::new()
    }
}

impl Progress for Spinner {
    fn start(&self, service: Service) {
        self.service.set(Some(service));
        self.bar.set_message(self.base_message());
        self.bar.enable_steady_tick(Duration::from_millis(100));
    }

    fn finish(&self) {
        self.bar.finish_and_clear();
    }
}

impl RetryObserver for Spinner {
    fn waiting(&self, wait: Wait) {
        if self.bar.is_hidden() {
            let service = self.service.get().map_or("The service", Service::name);
            anstream::eprintln!(
                "{service} {}; retrying in {}",
                Reason(wait.status),
                Seconds(wait.duration)
            );
        } else {
            let reason = if wait.status == 429 {
                "rate limited".to_owned()
            } else {
                format!("HTTP {}", wait.status)
            };
            self.bar.set_message(format!(
                "{} {reason}, retrying in {}",
                self.base_message(),
                Seconds(wait.duration)
            ));
        }
    }

    fn resumed(&self) {
        self.bar.set_message(self.base_message());
    }
}

/// Why a request is retried, as a sentence part: "is rate limiting requests".
struct Reason(u16);

impl fmt::Display for Reason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            429 => f.write_str("is rate limiting requests"),
            status => write!(f, "answered HTTP {status}"),
        }
    }
}

/// "4s", or "0.5s" for fractions.
struct Seconds(Duration);

impl fmt::Display for Seconds {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.0.subsec_millis() == 0 {
            write!(f, "{}s", self.0.as_secs())
        } else {
            write!(f, "{:.1}s", self.0.as_secs_f64())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_waits() {
        assert_eq!(Seconds(Duration::from_secs(4)).to_string(), "4s");
        assert_eq!(Seconds(Duration::from_millis(500)).to_string(), "0.5s");
        assert_eq!(Reason(429).to_string(), "is rate limiting requests");
        assert_eq!(Reason(503).to_string(), "answered HTTP 503");
    }
}
