//! The streaming services' APIs, each turned into a [`Catalog`](crate::catalog::Catalog).
//!
//! Both services use the OAuth 2.0 client-credentials flow (no user login):
//! the client ID and secret buy a short-lived access token, which then
//! authorizes the API requests.

pub mod spotify;
pub mod tidal;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde::Deserialize;

use crate::catalog::Include;
use crate::credentials::Credentials;
use crate::error::{Error, Result};
use crate::http::{self, Client};

/// What to fetch.
#[derive(Clone, Copy, Debug)]
pub struct Query<'a> {
    pub artist_id: &'a str,
    /// ISO 3166-1 alpha-2 country code, upper-case: catalogs differ by country.
    pub country: &'a str,
    pub include: Include,
}

/// Where a service's API lives. Tests point these at a local server.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Endpoints {
    /// OAuth token endpoint.
    pub token_url: String,
    /// Base URL of the API, without a trailing slash.
    pub api_base: String,
}

/// Upper bound on followed pagination links, in case a service keeps
/// returning a `next` link forever.
const MAX_PAGES: usize = 500;

#[derive(Deserialize)]
struct Token {
    access_token: String,
}

/// Runs the client-credentials flow (RFC 6749, section 4.4) and returns the access token.
fn fetch_access_token(
    client: &Client<'_>,
    token_url: &str,
    credentials: &Credentials,
) -> Result<String> {
    const CONTEXT: &str = "requesting an access token";
    let pair = format!("{}:{}", credentials.client_id, credentials.client_secret);
    let authorization = format!("Basic {}", BASE64.encode(pair));
    let response = client.post_form(
        token_url,
        &[
            ("Authorization", &authorization),
            ("Accept", "application/json"),
        ],
        &[("grant_type", "client_credentials")],
        CONTEXT,
    )?;

    match response.status {
        200..=299 => {}
        // OAuth servers answer 400 ("invalid_client") or 401 to bad credentials.
        400 | 401 => {
            return Err(Error::InvalidCredentials {
                service: client.service(),
                detail: http::error_detail(&response.body),
            });
        }
        _ => return Err(client.status_error(&response, CONTEXT)),
    }

    let token: Token = client.decode(&response.body, CONTEXT)?;
    if token.access_token.is_empty() {
        return Err(Error::UnexpectedResponse {
            service: client.service(),
            context: CONTEXT,
            reason: "the access token is empty".to_owned(),
        });
    }
    Ok(token.access_token)
}

/// Turns a 404 on the artist lookup into [`Error::ArtistNotFound`].
fn artist_not_found(err: Error, query: Query<'_>) -> Error {
    match err {
        Error::Status {
            service,
            status: 404,
            ..
        } => Error::ArtistNotFound {
            service,
            id: query.artist_id.to_owned(),
            country: query.country.to_owned(),
        },
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::Service;
    use crate::http::RetryPolicy;

    fn credentials() -> Credentials {
        Credentials {
            client_id: "id".into(),
            client_secret: "secret".into(),
        }
    }

    #[test]
    fn token_request_uses_basic_auth_and_a_form_body() {
        let mut server = mockito::Server::new();
        let mock = server
            .mock("POST", "/token")
            .match_header("authorization", "Basic aWQ6c2VjcmV0") // base64("id:secret")
            .match_header("content-type", "application/x-www-form-urlencoded")
            .match_body("grant_type=client_credentials")
            .with_body(r#"{"access_token":"tok","token_type":"Bearer","expires_in":3600}"#)
            .create();
        let client = Client::new(Service::Spotify, RetryPolicy::default(), &());

        let token = fetch_access_token(&client, &format!("{}/token", server.url()), &credentials())
            .unwrap();

        assert_eq!(token, "tok");
        mock.assert();
    }

    #[test]
    fn rejected_credentials_say_what_to_check() {
        let mut server = mockito::Server::new();
        server
            .mock("POST", "/token")
            .with_status(400)
            .with_body(r#"{"error":"invalid_client","error_description":"Invalid client"}"#)
            .create();
        let client = Client::new(Service::Spotify, RetryPolicy::default(), &());

        let err = fetch_access_token(&client, &format!("{}/token", server.url()), &credentials())
            .unwrap_err();

        assert_eq!(
            err.to_string(),
            "Spotify did not accept the client credentials: Invalid client\n\
             Check SPOTIFY_CLIENT_ID and SPOTIFY_CLIENT_SECRET."
        );
    }
}
