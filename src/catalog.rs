//! Service-neutral types for what the providers return.

use std::fmt;

use serde::{Deserialize, Serialize};

/// A supported streaming service.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Service {
    Spotify,
    Tidal,
}

impl Service {
    /// The service's name as its owner writes it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Spotify => "Spotify",
            Self::Tidal => "TIDAL",
        }
    }

    /// Prefix of the credential variables, e.g. `TIDAL` for `TIDAL_CLIENT_ID`.
    #[must_use]
    pub const fn env_prefix(self) -> &'static str {
        match self {
            Self::Spotify => "SPOTIFY",
            Self::Tidal => "TIDAL",
        }
    }

    /// Where developers register an app to get credentials.
    #[must_use]
    pub const fn developer_dashboard(self) -> &'static str {
        match self {
            Self::Spotify => "https://developer.spotify.com/dashboard",
            Self::Tidal => "https://developer.tidal.com/dashboard",
        }
    }
}

impl fmt::Display for Service {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// The kind of a release, as far as the services tell them apart.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ReleaseType {
    Album,
    Ep,
    Single,
    Compilation,
}

/// Which release types to list. Albums are always listed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Include {
    pub eps: bool,
    pub singles: bool,
    pub compilations: bool,
}

impl Include {
    /// Albums only (the default).
    pub const ALBUMS: Self = Self {
        eps: false,
        singles: false,
        compilations: false,
    };
    /// Every release type.
    pub const ALL: Self = Self {
        eps: true,
        singles: true,
        compilations: true,
    };

    /// Whether releases of type `release_type` should be listed.
    #[must_use]
    pub const fn contains(self, release_type: ReleaseType) -> bool {
        match release_type {
            ReleaseType::Album => true,
            ReleaseType::Ep => self.eps,
            ReleaseType::Single => self.singles,
            ReleaseType::Compilation => self.compilations,
        }
    }
}

/// One rendition of a cover image.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Image {
    pub url: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

impl Image {
    /// Pixel count, or 0 when the size is unknown.
    #[must_use]
    pub fn area(&self) -> u64 {
        u64::from(self.width.unwrap_or(0)) * u64::from(self.height.unwrap_or(0))
    }
}

/// The artist a catalog belongs to.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Artist {
    pub id: String,
    pub name: String,
    /// The artist's page on the service.
    pub url: String,
}

/// A release as listed by a service, before selection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Release {
    pub id: String,
    pub title: String,
    /// `YYYY`, `YYYY-MM` or `YYYY-MM-DD`, as the service reports it.
    pub release_date: String,
    pub release_type: ReleaseType,
    /// The release's page on the service.
    pub url: String,
    /// Cover renditions, in any order.
    pub images: Vec<Image>,
    /// False when the artist only appears on the release (e.g. a various-artists compilation).
    pub by_artist: bool,
}

/// Everything a provider found for one artist.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Catalog {
    pub service: Service,
    pub artist: Artist,
    pub releases: Vec<Release>,
}
