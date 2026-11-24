//! TIDAL API v2 (JSON:API).
//!
//! ```text
//! POST https://auth.tidal.com/v1/oauth2/token
//! GET  /v2/artists/{id}?countryCode=…&include=albums.coverArt
//! GET  /v2/artists/{id}/relationships/albums?countryCode=…&include=albums.coverArt&page[cursor]=…
//!      (repeated while there is a `next` link)
//! ```
//!
//! * Responses are JSON:API documents: related resources arrive in a shared
//!   `included` array and are joined by `type` and `id`.
//! * `include=albums.coverArt` returns the albums *and* their cover art in
//!   the same response. The artist request already carries the first page of
//!   albums, so most artists need a single API request.
//! * Pages hold 20 albums. There is no page-size parameter and no server-side
//!   album-type filter (`filter[albumType]` is rejected), so singles are paged
//!   through even when only albums are wanted.
//! * TIDAL allows a burst of about 10 requests, then answers HTTP 429 with
//!   `Retry-After` (4 s when observed). Keeping the request count low is the
//!   best defense; the HTTP client waits and retries.
//! * TIDAL has no "compilation" album type. Covers go up to 1280×1280 px.

use std::collections::HashMap;

use serde::Deserialize;

use super::{Endpoints, MAX_PAGES, Query, artist_not_found, fetch_access_token};
use crate::catalog::{Artist, Catalog, Image, Release, ReleaseType, Service};
use crate::credentials::Credentials;
use crate::error::{Error, Result};
use crate::http::Client;

/// Albums plus their cover art. TIDAL includes the intermediate `albums` of a
/// nested include, so `albums,albums.coverArt` isn't needed.
const INCLUDE: &str = "include=albums.coverArt";

impl Endpoints {
    /// The real TIDAL endpoints.
    #[must_use]
    pub fn tidal() -> Self {
        Self {
            token_url: "https://auth.tidal.com/v1/oauth2/token".to_owned(),
            api_base: "https://openapi.tidal.com/v2".to_owned(),
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
        ("Accept", "application/vnd.api+json"),
    ];

    // The artist document carries the first page of albums.
    let url = format!(
        "{}/artists/{}?countryCode={}&{INCLUDE}",
        endpoints.api_base, query.artist_id, query.country
    );
    let artist: ArtistDocument = client
        .get_json(&url, &headers, "fetching the artist")
        .map_err(|err| artist_not_found(err, query))?;
    let first_page = artist.data.relationships.albums;
    let mut releases = collect(&first_page.data, &artist.included, query);

    let mut next = next_page_url(endpoints, first_page.links);
    let mut pages = 1;
    while let Some(url) = next {
        if pages == MAX_PAGES {
            return Err(Error::UnexpectedResponse {
                service: Service::Tidal,
                context: "fetching albums",
                reason: format!("more than {MAX_PAGES} pages"),
            });
        }
        let page: RelationshipDocument = client.get_json(&url, &headers, "fetching albums")?;
        releases.extend(collect(&page.data, &page.included, query));
        next = next_page_url(endpoints, page.links);
        pages += 1;
    }

    Ok(Catalog {
        service: Service::Tidal,
        artist: Artist {
            id: query.artist_id.to_owned(),
            name: artist.data.attributes.name,
            url: share_link("artist", query.artist_id),
        },
        releases,
    })
}

/// The releases `identifiers` refer to, looked up in `included`, of the requested types.
fn collect(identifiers: &[Identifier], included: &[Included], query: Query<'_>) -> Vec<Release> {
    let mut albums = HashMap::new();
    let mut artworks = HashMap::new();
    for resource in included {
        match resource {
            Included::Albums {
                id,
                attributes,
                relationships,
            } => {
                albums.insert(id.as_str(), (attributes, relationships));
            }
            Included::Artworks { id, attributes } => {
                artworks.insert(id.as_str(), attributes);
            }
            Included::Other => {}
        }
    }

    identifiers
        .iter()
        .filter_map(|identifier| {
            let (attributes, relationships) = albums.get(identifier.id.as_str())?;
            let release_type = release_type(attributes.album_type.as_deref());
            if !query.include.contains(release_type) {
                return None;
            }
            // Animated covers (`VIDEO`) are skipped.
            let images = relationships
                .cover_art
                .data
                .iter()
                .filter_map(|artwork| artworks.get(artwork.id.as_str()))
                .filter(|artwork| {
                    artwork
                        .media_type
                        .as_deref()
                        .is_none_or(|kind| kind == "IMAGE")
                })
                .flat_map(|artwork| &artwork.files)
                .map(|file| Image {
                    url: file.href.clone(),
                    width: file.meta.as_ref().map(|size| size.width),
                    height: file.meta.as_ref().map(|size| size.height),
                })
                .collect();
            Some(Release {
                id: identifier.id.clone(),
                title: display_title(&attributes.title, attributes.version.as_deref()),
                release_date: attributes.release_date.clone().unwrap_or_default(),
                release_type,
                url: share_link("album", &identifier.id),
                images,
                // The albums relationship lists the artist's own releases.
                by_artist: true,
            })
        })
        .collect()
}

/// The absolute URL of the next page, if any.
///
/// TIDAL's links are relative to the API root (`/artists/…`). The link in the
/// artist document lacks the `include` parameter, so it is added back.
fn next_page_url(endpoints: &Endpoints, links: Links) -> Option<String> {
    let link = links.next?;
    let url = if link.starts_with("http://") || link.starts_with("https://") {
        link
    } else {
        format!("{}{link}", endpoints.api_base)
    };
    Some(if url.contains("include=") {
        url
    } else {
        format!("{url}&{INCLUDE}")
    })
}

/// A link in the form the TIDAL apps share, e.g. `https://tidal.com/album/17014237/u`.
/// (The API's own `TIDAL_SHARING` links use the older `/browse/album/…` form.)
fn share_link(kind: &str, id: &str) -> String {
    format!("https://tidal.com/{kind}/{id}/u")
}

/// The title plus TIDAL's separate version attribute, as the TIDAL apps show it:
/// "OK Computer" + "OKNOTOK 1997 2017" → "OK Computer (OKNOTOK 1997 2017)".
fn display_title(title: &str, version: Option<&str>) -> String {
    match version.map(str::trim) {
        Some(version)
            if !version.is_empty() && !title.to_lowercase().contains(&version.to_lowercase()) =>
        {
            format!("{title} ({version})")
        }
        _ => title.to_owned(),
    }
}

fn release_type(album_type: Option<&str>) -> ReleaseType {
    match album_type {
        Some(kind) if kind.eq_ignore_ascii_case("EP") => ReleaseType::Ep,
        Some(kind) if kind.eq_ignore_ascii_case("SINGLE") => ReleaseType::Single,
        _ => ReleaseType::Album,
    }
}

// JSON:API shapes: only the fields we use. Unknown fields are ignored.

#[derive(Deserialize)]
struct ArtistDocument {
    data: ArtistResource,
    #[serde(default)]
    included: Vec<Included>,
}

#[derive(Deserialize)]
struct ArtistResource {
    attributes: ArtistAttributes,
    #[serde(default)]
    relationships: ArtistRelationships,
}

#[derive(Deserialize)]
struct ArtistAttributes {
    name: String,
}

#[derive(Deserialize, Default)]
struct ArtistRelationships {
    #[serde(default)]
    albums: Relationship,
}

/// A page of a to-many relationship.
#[derive(Deserialize, Default)]
struct Relationship {
    #[serde(default)]
    data: Vec<Identifier>,
    #[serde(default)]
    links: Links,
}

#[derive(Deserialize)]
struct RelationshipDocument {
    #[serde(default)]
    data: Vec<Identifier>,
    #[serde(default)]
    included: Vec<Included>,
    #[serde(default)]
    links: Links,
}

#[derive(Deserialize, Default)]
struct Links {
    next: Option<String>,
}

#[derive(Deserialize)]
struct Identifier {
    id: String,
}

/// A resource from `included`, by its `type`.
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum Included {
    Albums {
        id: String,
        attributes: AlbumAttributes,
        #[serde(default)]
        relationships: AlbumRelationships,
    },
    Artworks {
        id: String,
        attributes: ArtworkAttributes,
    },
    #[serde(other)]
    Other,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AlbumAttributes {
    title: String,
    album_type: Option<String>,
    release_date: Option<String>,
    version: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct AlbumRelationships {
    #[serde(default)]
    cover_art: Relationship,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ArtworkAttributes {
    media_type: Option<String>,
    #[serde(default)]
    files: Vec<ArtworkFile>,
}

#[derive(Deserialize)]
struct ArtworkFile {
    href: String,
    meta: Option<Size>,
}

#[derive(Deserialize)]
struct Size {
    width: u32,
    height: u32,
}

#[cfg(test)]
mod tests {
    use mockito::{Mock, ServerGuard};
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::catalog::Include;
    use crate::http::{RetryObserver, RetryPolicy, Wait};

    const BEARER: &str = "Bearer test-tidal-token";
    const ACCEPT: &str = "application/vnd.api+json";
    /// The artist document's `next` link, with `include` added back.
    const PAGE_2: &str = "/v2/artists/4242/relationships/albums?countryCode=US&page%5Bcursor%5D=page2&include=albums.coverArt";

    fn fixture(name: &str) -> String {
        let path = format!("{}/tests/fixtures/tidal/{name}", env!("CARGO_MANIFEST_DIR"));
        std::fs::read_to_string(&path).unwrap_or_else(|err| panic!("{path}: {err}"))
    }

    fn mock_token_and_artist(server: &mut ServerGuard, country: &str) -> Vec<Mock> {
        vec![
            server
                .mock("POST", "/v1/oauth2/token")
                .with_body(fixture("token.json"))
                .create(),
            server
                .mock(
                    "GET",
                    format!("/v2/artists/4242?countryCode={country}&include=albums.coverArt")
                        .as_str(),
                )
                .match_header("authorization", BEARER)
                .match_header("accept", ACCEPT)
                .with_body(fixture("artist.json"))
                .create(),
        ]
    }

    fn mock_page_2(server: &mut ServerGuard) -> Mock {
        server
            .mock("GET", PAGE_2)
            .match_header("authorization", BEARER)
            .match_header("accept", ACCEPT)
            .with_body(fixture("albums-page-2.json"))
            .create()
    }

    fn fetch(
        server: &ServerGuard,
        country: &str,
        include: Include,
        observer: &dyn RetryObserver,
    ) -> Result<Catalog> {
        let endpoints = Endpoints {
            token_url: format!("{}/v1/oauth2/token", server.url()),
            api_base: format!("{}/v2", server.url()),
        };
        let credentials = Credentials {
            client_id: "id".into(),
            client_secret: "secret".into(),
        };
        let retry = RetryPolicy {
            backoff: std::time::Duration::ZERO,
            ..RetryPolicy::default()
        };
        let client = Client::new(Service::Tidal, retry, observer);
        fetch_catalog(
            &client,
            &endpoints,
            &credentials,
            Query {
                artist_id: "4242",
                country,
                include,
            },
        )
    }

    #[test]
    fn reads_albums_and_covers_from_the_artist_document_and_later_pages() {
        let mut server = mockito::Server::new();
        let mut mocks = mock_token_and_artist(&mut server, "US");
        mocks.push(mock_page_2(&mut server));

        let catalog = fetch(&server, "US", Include::ALBUMS, &()).unwrap();

        mocks.iter().for_each(Mock::assert);
        assert_eq!(catalog.service, Service::Tidal);
        assert_eq!(catalog.artist.name, "The Test Pilots");
        assert_eq!(catalog.artist.url, "https://tidal.com/artist/4242/u");

        // Albums only, in TIDAL's order (the order of `data`, not of `included`).
        let titles: Vec<_> = catalog.releases.iter().map(|r| r.title.as_str()).collect();
        assert_eq!(
            titles,
            [
                "First Light",
                "Night Drive (Deluxe Edition)",
                "Live at the Roundhouse"
            ]
        );

        let first_light = &catalog.releases[0];
        assert_eq!(first_light.release_date, "2015-04-10");
        assert_eq!(first_light.release_type, ReleaseType::Album);
        assert_eq!(first_light.url, "https://tidal.com/album/1001/u");
        assert_eq!(first_light.images.len(), 2);
        assert_eq!(
            first_light.images[1].url,
            "https://resources.tidal.com/images/first/light/1280x1280.jpg"
        );
        assert_eq!(first_light.images[1].width, Some(1280));

        // The animated cover is skipped; only the still image remains.
        let roundhouse = &catalog.releases[2];
        assert_eq!(roundhouse.images.len(), 1);
        assert_eq!(
            roundhouse.images[0].url,
            "https://resources.tidal.com/images/roundhouse/1280x1280.jpg"
        );
    }

    #[test]
    fn keeps_eps_and_singles_when_asked() {
        let mut server = mockito::Server::new();
        // The artist request carries the requested country; `next` links are followed as given.
        let mut mocks = mock_token_and_artist(&mut server, "DE");
        mocks.push(mock_page_2(&mut server));

        let catalog = fetch(&server, "DE", Include::ALL, &()).unwrap();

        mocks.iter().for_each(Mock::assert);
        let types: Vec<_> = catalog.releases.iter().map(|r| r.release_type).collect();
        assert_eq!(
            types,
            [
                ReleaseType::Album,
                ReleaseType::Album,
                ReleaseType::Album,
                ReleaseType::Ep,
                ReleaseType::Single
            ]
        );
        assert_eq!(catalog.releases[4].images, []); // no cover art
    }

    #[test]
    fn waits_out_rate_limiting() {
        struct Count(std::cell::Cell<u32>);
        impl RetryObserver for Count {
            fn waiting(&self, wait: Wait) {
                assert_eq!(wait.status, 429);
                self.0.set(self.0.get() + 1);
            }
        }

        let mut server = mockito::Server::new();
        let mut mocks = mock_token_and_artist(&mut server, "US");
        mocks.push(
            server
                .mock("GET", PAGE_2)
                .with_status(429)
                .with_header("retry-after", "0")
                .create(),
        );
        mocks.push(mock_page_2(&mut server));
        let waits = Count(std::cell::Cell::new(0));

        let catalog = fetch(&server, "US", Include::ALBUMS, &waits).unwrap();

        mocks.iter().for_each(Mock::assert);
        assert_eq!(catalog.releases.len(), 3);
        assert_eq!(waits.0.get(), 1);
    }

    #[test]
    fn reports_an_unknown_artist() {
        let mut server = mockito::Server::new();
        server
            .mock("POST", "/v1/oauth2/token")
            .with_body(fixture("token.json"))
            .create();
        server
            .mock(
                "GET",
                "/v2/artists/4242?countryCode=US&include=albums.coverArt",
            )
            .with_status(404)
            .with_body(r#"{"errors":[{"code":"NOT_FOUND","detail":"Artist not found"}]}"#)
            .create();

        let err = fetch(&server, "US", Include::ALBUMS, &()).unwrap_err();

        assert_eq!(
            err.to_string(),
            "TIDAL has no artist with ID 4242 (catalog country: US)"
        );
    }

    #[test]
    fn next_page_url_resolves_relative_links_and_restores_the_include() {
        let endpoints = Endpoints::tidal();
        let links = |next: &str| Links {
            next: Some(next.to_owned()),
        };
        assert_eq!(
            next_page_url(
                &endpoints,
                links("/artists/1/relationships/albums?countryCode=US&page%5Bcursor%5D=x")
            )
            .unwrap(),
            "https://openapi.tidal.com/v2/artists/1/relationships/albums?countryCode=US&page%5Bcursor%5D=x&include=albums.coverArt"
        );
        assert_eq!(
            next_page_url(
                &endpoints,
                links("/artists/1/relationships/albums?include=albums.coverArt&page%5Bcursor%5D=x")
            )
            .unwrap(),
            "https://openapi.tidal.com/v2/artists/1/relationships/albums?include=albums.coverArt&page%5Bcursor%5D=x"
        );
        assert_eq!(next_page_url(&endpoints, Links::default()), None);
    }

    #[test]
    fn display_title_appends_the_version_unless_the_title_has_it() {
        assert_eq!(display_title("Songs", None), "Songs");
        assert_eq!(display_title("Songs", Some(" ")), "Songs");
        assert_eq!(display_title("Songs", Some("Live")), "Songs (Live)");
        assert_eq!(display_title("Songs (Live)", Some("live")), "Songs (Live)");
    }

    #[test]
    fn release_types_follow_tidals_album_type() {
        assert_eq!(release_type(Some("ALBUM")), ReleaseType::Album);
        assert_eq!(release_type(Some("EP")), ReleaseType::Ep);
        assert_eq!(release_type(Some("SINGLE")), ReleaseType::Single);
        assert_eq!(release_type(Some("SOMETHING_NEW")), ReleaseType::Album);
        assert_eq!(release_type(None), ReleaseType::Album);
    }
}
