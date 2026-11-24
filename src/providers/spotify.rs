//! Spotify Web API.
//!
//! ```text
//! POST https://accounts.spotify.com/api/token
//! GET  /v1/artists/{id}
//! GET  /v1/artists/{id}/albums?include_groups=…&market=…&limit=10   (follows `next`)
//! ```
//!
//! * 10 is the documented maximum page size of the albums endpoint.
//! * `market` restricts results to one country's catalog, which also avoids
//!   most regional duplicates.
//! * Spotify reports EPs as `album_type: "single"`. Following Spotify's own
//!   definition (EPs have 4–6 tracks), singles with 4 or more tracks count as EPs.
//! * Covers come in 640, 300 and 64 px renditions.
//! * Since February 2026, Spotify only serves Development Mode apps whose
//!   owner has a Premium subscription.

use serde::Deserialize;

use super::{Endpoints, MAX_PAGES, Query, artist_not_found, fetch_access_token};
use crate::catalog::{Artist, Catalog, Image, Include, Release, ReleaseType, Service};
use crate::credentials::Credentials;
use crate::error::{Error, Result};
use crate::http::Client;

const PAGE_SIZE: u32 = 10;

impl Endpoints {
    /// The real Spotify endpoints.
    #[must_use]
    pub fn spotify() -> Self {
        Self {
            token_url: "https://accounts.spotify.com/api/token".to_owned(),
            api_base: "https://api.spotify.com/v1".to_owned(),
        }
    }
}

/// Fetches the artist and their releases of the requested types.
pub fn fetch_catalog(
    client: &Client<'_>,
    endpoints: &Endpoints,
    credentials: &Credentials,
    query: Query<'_>,
) -> Result<Catalog> {
    let token = fetch_access_token(client, &endpoints.token_url, credentials)?;
    let authorization = format!("Bearer {token}");
    let headers = [
        ("Authorization", authorization.as_str()),
        ("Accept", "application/json"),
    ];

    let url = format!("{}/artists/{}", endpoints.api_base, query.artist_id);
    let artist: ArtistObject = client
        .get_json(&url, &headers, "fetching the artist")
        .map_err(|err| artist_not_found(err, query))?;

    let mut releases = Vec::new();
    let mut next = Some(format!(
        "{}/artists/{}/albums?include_groups={}&market={}&limit={PAGE_SIZE}",
        endpoints.api_base,
        query.artist_id,
        include_groups(query.include),
        query.country,
    ));
    let mut pages = 0;
    while let Some(url) = next {
        if pages == MAX_PAGES {
            return Err(Error::UnexpectedResponse {
                service: Service::Spotify,
                context: "fetching albums",
                reason: format!("more than {MAX_PAGES} pages"),
            });
        }
        let page: AlbumPage = client.get_json(&url, &headers, "fetching albums")?;
        releases.extend(
            page.items
                .into_iter()
                .map(|album| album.into_release(query.artist_id)),
        );
        next = page.next;
        pages += 1;
    }

    Ok(Catalog {
        service: Service::Spotify,
        artist: Artist {
            id: query.artist_id.to_owned(),
            url: artist
                .external_urls
                .spotify
                .unwrap_or_else(|| format!("https://open.spotify.com/artist/{}", query.artist_id)),
            name: artist.name,
        },
        releases,
    })
}

/// Spotify's `include_groups` for the requested types. EPs and singles share the "single" group.
fn include_groups(include: Include) -> &'static str {
    match (include.eps || include.singles, include.compilations) {
        (true, true) => "album,single,compilation",
        (true, false) => "album,single",
        (false, true) => "album,compilation",
        (false, false) => "album",
    }
}

// JSON shapes: only the fields we use. Unknown fields are ignored.

#[derive(Deserialize, Default)]
struct ExternalUrls {
    spotify: Option<String>,
}

#[derive(Deserialize)]
struct ArtistObject {
    name: String,
    #[serde(default)]
    external_urls: ExternalUrls,
}

#[derive(Deserialize)]
struct AlbumPage {
    items: Vec<AlbumObject>,
    next: Option<String>,
}

#[derive(Deserialize)]
struct AlbumObject {
    id: String,
    name: String,
    album_type: Option<String>,
    #[serde(default)]
    total_tracks: u32,
    #[serde(default)]
    release_date: String,
    #[serde(default)]
    images: Vec<Image>,
    #[serde(default)]
    artists: Vec<ArtistRef>,
    #[serde(default)]
    external_urls: ExternalUrls,
}

#[derive(Deserialize)]
struct ArtistRef {
    id: Option<String>,
}

impl AlbumObject {
    fn into_release(self, artist_id: &str) -> Release {
        let release_type = match self.album_type.as_deref() {
            Some(kind) if kind.eq_ignore_ascii_case("compilation") => ReleaseType::Compilation,
            Some(kind) if kind.eq_ignore_ascii_case("single") && self.total_tracks >= 4 => {
                ReleaseType::Ep
            }
            Some(kind) if kind.eq_ignore_ascii_case("single") => ReleaseType::Single,
            _ => ReleaseType::Album,
        };
        Release {
            by_artist: self
                .artists
                .iter()
                .any(|artist| artist.id.as_deref() == Some(artist_id)),
            url: self
                .external_urls
                .spotify
                .unwrap_or_else(|| format!("https://open.spotify.com/album/{}", self.id)),
            id: self.id,
            title: self.name,
            release_date: self.release_date,
            release_type,
            images: self.images,
        }
    }
}

#[cfg(test)]
mod tests {
    use mockito::{Mock, ServerGuard};
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::http::RetryPolicy;

    const ARTIST_ID: &str = "0TestPilots00000000000";

    fn fixture(name: &str, server: &ServerGuard) -> String {
        let path = format!(
            "{}/tests/fixtures/spotify/{name}",
            env!("CARGO_MANIFEST_DIR")
        );
        let text = std::fs::read_to_string(&path).unwrap_or_else(|err| panic!("{path}: {err}"));
        // Pagination links are absolute; point them at the test server.
        text.replace(
            "https://api.spotify.com/v1",
            &format!("{}/v1", server.url()),
        )
    }

    fn endpoints(server: &ServerGuard) -> Endpoints {
        Endpoints {
            token_url: format!("{}/api/token", server.url()),
            api_base: format!("{}/v1", server.url()),
        }
    }

    fn credentials() -> Credentials {
        Credentials {
            client_id: "id".into(),
            client_secret: "secret".into(),
        }
    }

    /// Mocks the whole conversation for an artist with two pages of albums.
    fn mock_happy_path(server: &mut ServerGuard, first_page_query: &str) -> Vec<Mock> {
        let token = fixture("token.json", server);
        let artist = fixture("artist.json", server);
        let page_1 = fixture("albums-page-1.json", server);
        let page_2 = fixture("albums-page-2.json", server);
        let bearer = "Bearer test-spotify-token";
        vec![
            server.mock("POST", "/api/token").with_body(token).create(),
            server.mock("GET", format!("/v1/artists/{ARTIST_ID}").as_str()).match_header("authorization", bearer).with_body(artist).create(),
            server
                .mock("GET", format!("/v1/artists/{ARTIST_ID}/albums?{first_page_query}").as_str())
                .match_header("authorization", bearer)
                .with_body(page_1)
                .create(),
            server
                .mock("GET", format!("/v1/artists/{ARTIST_ID}/albums?offset=10&limit=10&include_groups=album&market=US").as_str())
                .match_header("authorization", bearer)
                .with_body(page_2)
                .create(),
        ]
    }

    fn fetch(server: &ServerGuard, country: &str, include: Include) -> Result<Catalog> {
        let client = Client::new(Service::Spotify, RetryPolicy::default(), &());
        fetch_catalog(
            &client,
            &endpoints(server),
            &credentials(),
            Query {
                artist_id: ARTIST_ID,
                country,
                include,
            },
        )
    }

    #[test]
    fn follows_pagination_and_maps_every_release() {
        let mut server = mockito::Server::new();
        let mocks = mock_happy_path(&mut server, "include_groups=album&market=US&limit=10");

        let catalog = fetch(&server, "US", Include::ALBUMS).unwrap();

        mocks.iter().for_each(Mock::assert);
        assert_eq!(catalog.service, Service::Spotify);
        assert_eq!(catalog.artist.name, "The Test Pilots");
        assert_eq!(
            catalog.artist.url,
            "https://open.spotify.com/artist/0TestPilots00000000000"
        );
        assert_eq!(catalog.releases.len(), 7);

        let deluxe = &catalog.releases[0];
        assert_eq!(deluxe.title, "Night Drive (Deluxe Edition)");
        assert_eq!(deluxe.release_date, "2021-03-05");
        assert_eq!(deluxe.release_type, ReleaseType::Album);
        assert_eq!(
            deluxe.url,
            "https://open.spotify.com/album/1NightDriveDeluxe00000"
        );
        assert_eq!(deluxe.images.len(), 3);
        assert_eq!(deluxe.images[0].width, Some(640));
        assert!(deluxe.by_artist);

        // No external URL in the response: built from the ID.
        assert_eq!(
            catalog.releases[3].url,
            "https://open.spotify.com/album/1FirstLight00000000000"
        );
        assert_eq!(catalog.releases[4].release_type, ReleaseType::Ep); // a "single" with 5 tracks
        assert_eq!(catalog.releases[5].release_type, ReleaseType::Single); // a "single" with 1 track
        assert_eq!(catalog.releases[5].images[0].width, None);

        let sampler = &catalog.releases[6];
        assert_eq!(sampler.release_type, ReleaseType::Compilation);
        assert!(!sampler.by_artist); // only "Various Artists"
    }

    #[test]
    fn asks_for_the_requested_groups_and_country() {
        let mut server = mockito::Server::new();
        let mocks = mock_happy_path(
            &mut server,
            "include_groups=album,single,compilation&market=DE&limit=10",
        );

        fetch(&server, "DE", Include::ALL).unwrap();

        mocks.iter().for_each(Mock::assert);
    }

    #[test]
    fn include_groups_covers_every_combination() {
        assert_eq!(include_groups(Include::ALBUMS), "album");
        assert_eq!(
            include_groups(Include {
                eps: true,
                ..Include::ALBUMS
            }),
            "album,single"
        );
        assert_eq!(
            include_groups(Include {
                singles: true,
                ..Include::ALBUMS
            }),
            "album,single"
        );
        assert_eq!(
            include_groups(Include {
                compilations: true,
                ..Include::ALBUMS
            }),
            "album,compilation"
        );
        assert_eq!(include_groups(Include::ALL), "album,single,compilation");
    }

    #[test]
    fn reports_an_unknown_artist() {
        let mut server = mockito::Server::new();
        server
            .mock("POST", "/api/token")
            .with_body(fixture("token.json", &server))
            .create();
        server
            .mock("GET", format!("/v1/artists/{ARTIST_ID}").as_str())
            .with_status(404)
            .with_body(r#"{"error":{"status":404,"message":"Resource not found"}}"#)
            .create();

        let err = fetch(&server, "US", Include::ALBUMS).unwrap_err();

        assert_eq!(
            err.to_string(),
            "Spotify has no artist with ID 0TestPilots00000000000 (catalog country: US)"
        );
    }

    #[test]
    fn quotes_spotifys_explanation_for_other_errors() {
        let mut server = mockito::Server::new();
        server
            .mock("POST", "/api/token")
            .with_body(fixture("token.json", &server))
            .create();
        server
            .mock("GET", format!("/v1/artists/{ARTIST_ID}").as_str())
            .with_status(403)
            .with_body(r#"{"error":{"status":403,"message":"Active premium subscription required for the owner of the app."}}"#)
            .create();

        let err = fetch(&server, "US", Include::ALBUMS).unwrap_err();

        assert_eq!(
            err.to_string(),
            "Spotify answered HTTP 403 while fetching the artist: Active premium subscription required for the owner of the app."
        );
    }
}
