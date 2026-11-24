//! A small JSON-over-HTTP client with the policies both services need:
//!
//! * retries for rate limiting (HTTP 429, honoring `Retry-After`) and server
//!   errors (5xx), with exponential backoff, reported to a [`RetryObserver`];
//! * failures turned into [`Error`]s that quote the service's own explanation;
//! * lenient JSON decoding: unknown fields are ignored, so new API fields never break us.

use std::thread;
use std::time::Duration;

use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::catalog::Service;
use crate::error::{Error, Result};

/// How persistent to be with failing requests.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RetryPolicy {
    /// Total attempts per request, including the first one.
    pub max_attempts: u32,
    /// The longest `Retry-After` worth waiting for; longer waits fail right away.
    pub max_wait: Duration,
    /// Delay before the first retry when the server doesn't say; doubles each time.
    pub backoff: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 4,
            max_wait: Duration::from_secs(30),
            backoff: Duration::from_millis(500),
        }
    }
}

/// A pause before retrying a request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Wait {
    /// The status that caused it: 429 (rate limited) or 5xx.
    pub status: u16,
    pub duration: Duration,
}

/// Gets told about retry waits, so the user knows why nothing happens.
pub trait RetryObserver {
    /// About to sleep before retrying.
    fn waiting(&self, _wait: Wait) {}
    /// Done sleeping; the request is sent again.
    fn resumed(&self) {}
}

/// `()` ignores all notifications.
impl RetryObserver for () {}

/// A response with its body read.
#[derive(Debug)]
pub(crate) struct Response {
    pub status: u16,
    pub body: String,
    pub retry_after: Option<Duration>,
}

impl Response {
    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }
}

/// An HTTP client for one service.
pub struct Client<'a> {
    agent: ureq::Agent,
    service: Service,
    retry: RetryPolicy,
    observer: &'a dyn RetryObserver,
}

impl<'a> Client<'a> {
    #[must_use]
    pub fn new(service: Service, retry: RetryPolicy, observer: &'a dyn RetryObserver) -> Self {
        let agent = ureq::Agent::config_builder()
            // Error statuses are responses too: we need their headers and bodies.
            .http_status_as_error(false)
            .timeout_global(Some(Duration::from_secs(30)))
            .user_agent(concat!("album-covers/", env!("CARGO_PKG_VERSION")))
            .build()
            .into();
        Self {
            agent,
            service,
            retry,
            observer,
        }
    }

    /// The service this client talks to.
    #[must_use]
    pub const fn service(&self) -> Service {
        self.service
    }

    /// Sends a GET request and decodes the JSON response.
    /// `context` completes the sentence "… while {context}" in error messages.
    pub(crate) fn get_json<T: DeserializeOwned>(
        &self,
        url: &str,
        headers: &[(&str, &str)],
        context: &'static str,
    ) -> Result<T> {
        let response = self.send(context, || {
            headers
                .iter()
                .fold(self.agent.get(url), |request, (name, value)| {
                    request.header(*name, *value)
                })
                .call()
        })?;
        if !response.is_success() {
            return Err(self.status_error(&response, context));
        }
        self.decode(&response.body, context)
    }

    /// Sends a form POST and returns the response whatever its status, so the
    /// caller can interpret specific failures. Retries still apply.
    pub(crate) fn post_form(
        &self,
        url: &str,
        headers: &[(&str, &str)],
        form: &[(&str, &str)],
        context: &'static str,
    ) -> Result<Response> {
        self.send(context, || {
            headers
                .iter()
                .fold(self.agent.post(url), |request, (name, value)| {
                    request.header(*name, *value)
                })
                .send_form(form.iter().copied())
        })
    }

    /// Decodes a JSON body.
    pub(crate) fn decode<T: DeserializeOwned>(
        &self,
        body: &str,
        context: &'static str,
    ) -> Result<T> {
        serde_json::from_str(body).map_err(|err| Error::UnexpectedResponse {
            service: self.service,
            context,
            reason: err.to_string(),
        })
    }

    /// The error for an unsuccessful response.
    pub(crate) fn status_error(&self, response: &Response, context: &'static str) -> Error {
        if response.status == 429 {
            return Error::RateLimited {
                service: self.service,
                context,
                retry_after: response.retry_after,
            };
        }
        Error::Status {
            service: self.service,
            status: response.status,
            context,
            detail: error_detail(&response.body),
        }
    }

    fn send(
        &self,
        context: &'static str,
        attempt: impl Fn() -> Result<ureq::http::Response<ureq::Body>, ureq::Error>,
    ) -> Result<Response> {
        let network_error = |err| Error::Network {
            context,
            source: Box::new(err),
        };
        let mut backoff = self.retry.backoff;
        for attempt_number in 1.. {
            let response = attempt().and_then(read_response).map_err(network_error)?;

            let retryable = response.status == 429 || response.status >= 500;
            let wait = response.retry_after.unwrap_or(backoff);
            let worth_waiting = wait <= self.retry.max_wait;
            if response.is_success()
                || !retryable
                || !worth_waiting
                || attempt_number >= self.retry.max_attempts
            {
                return Ok(response);
            }

            self.observer.waiting(Wait {
                status: response.status,
                duration: wait,
            });
            thread::sleep(wait);
            self.observer.resumed();
            backoff = backoff.saturating_mul(2);
        }
        unreachable!("the attempt loop only ends by returning")
    }
}

fn read_response(mut response: ureq::http::Response<ureq::Body>) -> Result<Response, ureq::Error> {
    let retry_after = response
        .headers()
        .get("retry-after")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.trim().parse().ok())
        .map(Duration::from_secs);
    Ok(Response {
        status: response.status().as_u16(),
        body: response.body_mut().read_to_string()?,
        retry_after,
    })
}

/// Extracts a readable message from the common error body shapes:
///
/// | Source            | Shape                                                  |
/// | ----------------- | ------------------------------------------------------ |
/// | Spotify Web API   | `{"error": {"status": 404, "message": "…"}}`           |
/// | OAuth endpoints   | `{"error": "invalid_client", "error_description": "…"}` |
/// | TIDAL (JSON:API)  | `{"errors": [{"code": "…", "detail": "…"}]}`           |
///
/// Anything else is returned as trimmed text, at most 300 characters.
#[must_use]
pub fn error_detail(body: &str) -> String {
    let from_json = serde_json::from_str::<Value>(body).ok().and_then(|json| {
        let text = |value: &Value| {
            value
                .as_str()
                .filter(|text| !text.is_empty())
                .map(str::to_owned)
        };
        if let Some(error) = json.get("error") {
            return error
                .get("message")
                .and_then(text)
                .or_else(|| json.get("error_description").and_then(text))
                .or_else(|| text(error));
        }
        let first = json.get("errors")?.get(0)?;
        first
            .get("detail")
            .and_then(text)
            .or_else(|| first.get("code").and_then(text))
    });
    from_json.unwrap_or_else(|| body.trim().chars().take(300).collect())
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use pretty_assertions::assert_eq;

    use super::*;

    const NO_WAIT: RetryPolicy = RetryPolicy {
        max_attempts: 3,
        max_wait: Duration::from_secs(1),
        backoff: Duration::ZERO,
    };

    #[derive(Default)]
    struct Recorder(RefCell<Vec<String>>);

    impl RetryObserver for Recorder {
        fn waiting(&self, wait: Wait) {
            self.0
                .borrow_mut()
                .push(format!("wait {} {:?}", wait.status, wait.duration));
        }
        fn resumed(&self) {
            self.0.borrow_mut().push("resume".to_owned());
        }
    }

    #[derive(serde::Deserialize, Debug, PartialEq)]
    struct Answer {
        n: u32,
    }

    #[test]
    fn retries_rate_limits_and_server_errors() {
        let mut server = mockito::Server::new();
        let limited = server
            .mock("GET", "/x")
            .with_status(429)
            .with_header("Retry-After", "0")
            .create();
        let failed = server.mock("GET", "/x").with_status(503).create();
        let ok = server
            .mock("GET", "/x")
            .match_header("accept", "application/json")
            .with_body(r#"{"n":1}"#)
            .create();
        let recorder = Recorder::default();
        let client = Client::new(Service::Tidal, NO_WAIT, &recorder);

        let answer: Answer = client
            .get_json(
                &format!("{}/x", server.url()),
                &[("Accept", "application/json")],
                "testing",
            )
            .unwrap();

        assert_eq!(answer, Answer { n: 1 });
        for mock in [limited, failed, ok] {
            mock.assert();
        }
        assert_eq!(
            *recorder.0.borrow(),
            ["wait 429 0ns", "resume", "wait 503 0ns", "resume"]
        );
    }

    #[test]
    fn gives_up_after_max_attempts_with_the_services_explanation() {
        let mut server = mockito::Server::new();
        let mock = server
            .mock("GET", "/x")
            .with_status(500)
            .with_body(r#"{"error":{"status":500,"message":"Server error"}}"#)
            .expect(3)
            .create();
        let client = Client::new(Service::Spotify, NO_WAIT, &());

        let err = client
            .get_json::<Answer>(&format!("{}/x", server.url()), &[], "fetching x")
            .unwrap_err();

        assert_eq!(
            err.to_string(),
            "Spotify answered HTTP 500 while fetching x: Server error"
        );
        mock.assert();
    }

    #[test]
    fn fails_fast_when_retry_after_is_too_long() {
        let mut server = mockito::Server::new();
        let mock = server
            .mock("GET", "/x")
            .with_status(429)
            .with_header("Retry-After", "3600")
            .expect(1)
            .create();
        let client = Client::new(Service::Tidal, NO_WAIT, &());

        let err = client
            .get_json::<Answer>(&format!("{}/x", server.url()), &[], "fetching x")
            .unwrap_err();

        assert_eq!(
            err.to_string(),
            "TIDAL rate limit reached while fetching x; try again in 3600s"
        );
        mock.assert();
    }

    #[test]
    fn does_not_retry_client_errors() {
        let mut server = mockito::Server::new();
        let mock = server
            .mock("GET", "/x")
            .with_status(401)
            .with_body(r#"{"errors":[{"code":"UNAUTHORIZED","detail":"Invalid token"}]}"#)
            .expect(1)
            .create();
        let client = Client::new(Service::Tidal, NO_WAIT, &());

        let err = client
            .get_json::<Answer>(&format!("{}/x", server.url()), &[], "fetching x")
            .unwrap_err();

        assert_eq!(
            err.to_string(),
            "TIDAL answered HTTP 401 while fetching x: Invalid token"
        );
        mock.assert();
    }

    #[test]
    fn reports_unexpected_json() {
        let mut server = mockito::Server::new();
        server
            .mock("GET", "/x")
            .with_body(r#"{"n":"one"}"#)
            .create();
        let client = Client::new(Service::Tidal, NO_WAIT, &());

        let err = client
            .get_json::<Answer>(&format!("{}/x", server.url()), &[], "reading n")
            .unwrap_err();

        assert!(
            err.to_string()
                .starts_with("unexpected response from TIDAL while reading n: invalid type"),
            "{err}"
        );
    }

    #[test]
    fn reports_network_failures() {
        // Nothing listens on port 9 (discard) on test machines.
        let client = Client::new(Service::Tidal, NO_WAIT, &());
        let err = client
            .get_json::<Answer>("http://127.0.0.1:9/x", &[], "fetching x")
            .unwrap_err();
        assert!(
            err.to_string()
                .starts_with("network error while fetching x: "),
            "{err}"
        );
    }

    #[test]
    fn error_detail_understands_the_common_shapes() {
        assert_eq!(
            error_detail(r#"{"error":{"status":400,"message":"Invalid id"}}"#),
            "Invalid id"
        );
        assert_eq!(
            error_detail(r#"{"error":"invalid_client","error_description":"Invalid client"}"#),
            "Invalid client"
        );
        assert_eq!(
            error_detail(r#"{"error":"invalid_client"}"#),
            "invalid_client"
        );
        assert_eq!(
            error_detail(r#"{"errors":[{"code":"X","detail":"Bad token"}]}"#),
            "Bad token"
        );
        assert_eq!(
            error_detail(r#"{"errors":[{"code":"NOT_FOUND"}]}"#),
            "NOT_FOUND"
        );
        assert_eq!(error_detail("  Forbidden\n"), "Forbidden");
        assert_eq!(error_detail(""), "");
        assert_eq!(error_detail(&"é".repeat(400)).chars().count(), 300);
    }
}
