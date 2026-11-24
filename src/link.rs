//! Recognizes artist links.
//!
//! Accepted forms (the scheme is optional; query strings and fragments are ignored):
//!
//! | Service | Forms |
//! | ------- | ----- |
//! | Spotify | `https://open.spotify.com/artist/<id>`, `https://open.spotify.com/intl-de/artist/<id>`, `spotify:artist:<id>` |
//! | TIDAL   | `https://tidal.com/artist/<id>`, `https://tidal.com/browse/artist/<id>`, `https://listen.tidal.com/artist/<id>` |
//!
//! Spotify IDs are base-62 strings; TIDAL IDs are decimal numbers.

use std::str::FromStr;

use crate::catalog::Service;

/// Example links shown in error messages.
pub const EXAMPLES: &str =
    "https://tidal.com/artist/3974492 or https://open.spotify.com/artist/4Z8W4fKeB5YxbusRsdQVPb";

/// A parsed artist link.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArtistLink {
    pub service: Service,
    pub id: String,
}

/// Why a string is not an artist link.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum LinkError {
    #[error("not a Spotify or TIDAL artist link (try something like {EXAMPLES})")]
    Unsupported,
    #[error("this link does not point to an artist page (try something like {EXAMPLES})")]
    NotAnArtist,
    #[error("the artist ID in this link looks malformed")]
    InvalidId,
}

impl FromStr for ArtistLink {
    type Err = LinkError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let text = input.trim();

        if let Some(rest) = text.strip_prefix("spotify:") {
            let (kind, id) = rest.split_once(':').ok_or(LinkError::NotAnArtist)?;
            if kind != "artist" {
                return Err(LinkError::NotAnArtist);
            }
            return Self::new(Service::Spotify, id);
        }

        let (host, path) = split_url(text);
        let service = if host.eq_ignore_ascii_case("open.spotify.com") {
            Service::Spotify
        } else if ["tidal.com", "www.tidal.com", "listen.tidal.com"]
            .iter()
            .any(|known| host.eq_ignore_ascii_case(known))
        {
            Service::Tidal
        } else {
            return Err(LinkError::Unsupported);
        };

        let mut segments = path.split('/').filter(|segment| !segment.is_empty());
        let mut first = segments.next().ok_or(LinkError::NotAnArtist)?;
        // Skip locale prefixes ("/intl-de/artist/…") and TIDAL's older "/browse/artist/…".
        if first.starts_with("intl-") || first == "browse" {
            first = segments.next().ok_or(LinkError::NotAnArtist)?;
        }
        if first != "artist" {
            return Err(LinkError::NotAnArtist);
        }
        Self::new(service, segments.next().unwrap_or_default())
    }
}

impl ArtistLink {
    fn new(service: Service, id: &str) -> Result<Self, LinkError> {
        let valid = match service {
            Service::Spotify => id.bytes().all(|c| c.is_ascii_alphanumeric()),
            Service::Tidal => id.bytes().all(|c| c.is_ascii_digit()),
        };
        if id.is_empty() || !valid {
            return Err(LinkError::InvalidId);
        }
        Ok(Self {
            service,
            id: id.to_owned(),
        })
    }
}

/// Splits a URL (with or without scheme) into host and path, dropping
/// userinfo, port, query and fragment.
fn split_url(text: &str) -> (&str, &str) {
    let rest = text.split_once("://").map_or(text, |(_, rest)| rest);
    let rest = rest.split(['?', '#']).next().unwrap_or_default();
    let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
    let host = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    let host = host.split_once(':').map_or(host, |(host, _)| host);
    (host, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(input: &str) -> Result<(Service, String), LinkError> {
        input
            .parse::<ArtistLink>()
            .map(|link| (link.service, link.id))
    }

    #[test]
    fn parses_spotify_artist_links() {
        let id = "7kWnE981vITXDnAD2cZmCV";
        for input in [
            "https://open.spotify.com/artist/7kWnE981vITXDnAD2cZmCV",
            "https://open.spotify.com/artist/7kWnE981vITXDnAD2cZmCV?si=abc123",
            "https://open.spotify.com/intl-de/artist/7kWnE981vITXDnAD2cZmCV",
            "open.spotify.com/artist/7kWnE981vITXDnAD2cZmCV/",
            "spotify:artist:7kWnE981vITXDnAD2cZmCV",
        ] {
            assert_eq!(
                parse(input),
                Ok((Service::Spotify, id.to_owned())),
                "{input}"
            );
        }
        assert_eq!(
            parse("  http://OPEN.SPOTIFY.COM/artist/abc#top \n"),
            Ok((Service::Spotify, "abc".to_owned()))
        );
    }

    #[test]
    fn parses_tidal_artist_links() {
        for input in [
            "https://tidal.com/artist/3346",
            "https://tidal.com/artist/3346/u",
            "https://tidal.com/browse/artist/3346",
            "https://listen.tidal.com/artist/3346",
            "https://www.tidal.com/browse/artist/3346?u",
            "tidal.com/artist/3346/albums",
        ] {
            assert_eq!(
                parse(input),
                Ok((Service::Tidal, "3346".to_owned())),
                "{input}"
            );
        }
    }

    #[test]
    fn rejects_links_to_other_sites() {
        for input in [
            "not a url",
            "",
            "https://example.com/artist/7kWnE981vITXDnAD2cZmCV",
            "https://open.spotify.com.evil.example/artist/abc",
            "https://nottidal.com/artist/1",
        ] {
            assert_eq!(parse(input), Err(LinkError::Unsupported), "{input}");
        }
    }

    #[test]
    fn rejects_links_to_other_pages() {
        for input in [
            "https://open.spotify.com/album/7kWnE981vITXDnAD2cZmCV",
            "https://tidal.com/browse/album/12345",
            "https://open.spotify.com/",
            "spotify:album:abc",
            "spotify:artist",
        ] {
            assert_eq!(parse(input), Err(LinkError::NotAnArtist), "{input}");
        }
    }

    #[test]
    fn rejects_malformed_ids() {
        for input in [
            "https://open.spotify.com/artist/not-valid_underscore",
            "https://open.spotify.com/artist",
            "https://tidal.com/artist/abc",
            "spotify:artist:",
        ] {
            assert_eq!(parse(input), Err(LinkError::InvalidId), "{input}");
        }
    }
}
