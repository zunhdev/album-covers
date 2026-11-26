//! Turns a raw catalog into the list of albums to print.
//!
//! [`select`] is pure and deterministic:
//!
//! 1. drop releases the artist only appears on, and release types the user
//!    did not ask for;
//! 2. pick the largest cover of each release (releases without one are dropped);
//! 3. split albums into studio and live recordings by title;
//! 4. drop duplicates: services often list the same album several times
//!    (regional editions, "Deluxe" or "Remastered" re-releases on the same day, and
//!    on TIDAL one copy per audio format). The copy with the best audio stays:
//!    hi-res, then lossless, then Dolby Atmos; on a tie, the first one listed;
//! 5. sort by release date, then by title.
//!
//! Text handling is ASCII-only on purpose: the heuristics only look for English
//! keywords, and other characters pass through untouched.

use std::cmp::Ordering;
use std::collections::HashMap;
use std::collections::hash_map::Entry;

use serde::Serialize;

use crate::catalog::{Formats, Image, Include, Release, ReleaseType};

/// The output group of an album.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    StudioAlbum,
    LiveAlbum,
    Ep,
    Single,
    Compilation,
}

impl Category {
    /// All categories, in output order.
    pub const ALL: [Self; 5] = [
        Self::StudioAlbum,
        Self::LiveAlbum,
        Self::Ep,
        Self::Single,
        Self::Compilation,
    ];

    /// Heading of the group in text output.
    #[must_use]
    pub const fn heading(self) -> &'static str {
        match self {
            Self::StudioAlbum => "Studio albums",
            Self::LiveAlbum => "Live albums",
            Self::Ep => "EPs",
            Self::Single => "Singles",
            Self::Compilation => "Compilations",
        }
    }

    /// "1 studio album", "2 studio albums", …
    #[must_use]
    pub fn count(self, n: usize) -> String {
        let (one, many) = match self {
            Self::StudioAlbum => ("studio album", "studio albums"),
            Self::LiveAlbum => ("live album", "live albums"),
            Self::Ep => ("EP", "EPs"),
            Self::Single => ("single", "singles"),
            Self::Compilation => ("compilation", "compilations"),
        };
        format!("{n} {}", if n == 1 { one } else { many })
    }
}

/// A tag for an album's audio formats, shown next to its title (TIDAL only).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Badge {
    Lossless,
    Hires,
    Atmos,
}

impl Badge {
    /// The badge's text in text output.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Lossless => "Lossless",
            Self::Hires => "Hi-Res",
            Self::Atmos => "Atmos",
        }
    }

    /// The best stereo quality (hi-res implies lossless), then Atmos if there's a mix.
    #[must_use]
    pub fn for_formats(formats: Formats) -> Vec<Self> {
        let stereo = if formats.hires {
            Some(Self::Hires)
        } else if formats.lossless {
            Some(Self::Lossless)
        } else {
            None
        };
        stereo
            .into_iter()
            .chain(formats.atmos.then_some(Self::Atmos))
            .collect()
    }
}

/// An album selected for output.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Album {
    pub title: String,
    pub release_date: String,
    pub category: Category,
    /// Audio format badges; empty when the service doesn't say (Spotify).
    pub badges: Vec<Badge>,
    /// The album's page on the service.
    pub url: String,
    /// The largest cover rendition.
    pub cover: Image,
}

impl Album {
    /// The year of release (the first four characters of the date).
    #[must_use]
    pub fn year(&self) -> &str {
        self.release_date.get(..4).unwrap_or(&self.release_date)
    }
}

/// Applies the whole pipeline described in the module docs.
#[must_use]
pub fn select(releases: Vec<Release>, include: Include) -> Vec<Album> {
    // Where each album is in `albums`, by duplicate key.
    let mut seen: HashMap<(String, String, Category), usize> = HashMap::new();
    let mut albums: Vec<(Formats, Album)> = Vec::new();
    for release in releases {
        if !release.by_artist || !include.contains(release.release_type) {
            continue;
        }
        let Some(cover) = best_cover(&release.images).cloned() else {
            continue;
        };
        let category = categorize(&release);
        let key = (
            normalize_title(&release.title),
            release.release_date.clone(),
            category,
        );
        let formats = release.formats;
        let album = Album {
            title: release.title,
            release_date: release.release_date,
            category,
            badges: Badge::for_formats(formats),
            url: release.url,
            cover,
        };
        match seen.entry(key) {
            Entry::Vacant(entry) => {
                entry.insert(albums.len());
                albums.push((formats, album));
            }
            Entry::Occupied(entry) => {
                let kept = &mut albums[*entry.get()];
                if preference(formats) > preference(kept.0) {
                    *kept = (formats, album);
                }
            }
        }
    }
    let mut albums: Vec<Album> = albums.into_iter().map(|(_, album)| album).collect();
    albums.sort_by(chronological);
    albums
}

/// How much a copy's audio is preferred: hi-res, then lossless, then Dolby Atmos.
/// So a hi-res stereo copy beats an Atmos-only one.
const fn preference(formats: Formats) -> (bool, bool, bool) {
    (formats.hires, formats.lossless, formats.atmos)
}

/// The largest image, or the first one when sizes are unknown or tied.
#[must_use]
pub fn best_cover(images: &[Image]) -> Option<&Image> {
    // `max_by_key` keeps the *last* maximum, so search from the end.
    images.iter().rev().max_by_key(|image| image.area())
}

/// Maps a release onto its output group.
#[must_use]
pub fn categorize(release: &Release) -> Category {
    match release.release_type {
        ReleaseType::Album if is_likely_live(&release.title) => Category::LiveAlbum,
        ReleaseType::Album => Category::StudioAlbum,
        ReleaseType::Ep => Category::Ep,
        ReleaseType::Single => Category::Single,
        ReleaseType::Compilation => Category::Compilation,
    }
}

/// Normalizes a title for duplicate detection: lower-cases ASCII letters,
/// removes closed `(…)` and `[…]` qualifiers, turns en and em dashes into `-`,
/// and collapses whitespace.
///
/// ```
/// # use album_covers::albums::normalize_title;
/// assert_eq!(normalize_title("First Album  (Deluxe) – Remastered"), "first album - remastered");
/// ```
#[must_use]
pub fn normalize_title(title: &str) -> String {
    let mut out = String::with_capacity(title.len());
    let mut pending_space = false;
    for segment in segments(title) {
        let Segment::Text(text) = segment else {
            pending_space = true;
            continue;
        };
        for c in text.chars() {
            if c.is_ascii_whitespace() {
                pending_space = true;
                continue;
            }
            if pending_space && !out.is_empty() {
                out.push(' ');
            }
            pending_space = false;
            out.push(match c {
                '\u{2013}' | '\u{2014}' => '-',
                c => c.to_ascii_lowercase(),
            });
        }
    }
    out
}

/// Guesses whether an album title names a live recording.
///
/// The main title counts as live when it contains one of the words *live*,
/// *unplugged*, *concert*, *session(s)* or the phrase *on stage*, as in
/// "Live at Wembley", "MTV Unplugged" or "In Concert".
///
/// A `(…)` or `[…]` qualifier counts only when it *starts* with *live* or
/// *unplugged*, or mentions *unplugged* or *in concert*. So "Hits (Live)" is
/// live, but "Hits (Deluxe Edition with Live Bonus Tracks)" is not.
///
/// This is a heuristic: a studio album called "Live Through This" is reported as live.
#[must_use]
pub fn is_likely_live(title: &str) -> bool {
    let main_words = segments(title).filter_map(|segment| match segment {
        Segment::Text(text) => Some(words(text)),
        Segment::Qualifier(_) => None,
    });
    let mut previous = "";
    for word in main_words.flatten() {
        if is_any(
            word,
            &["live", "unplugged", "concert", "session", "sessions"],
        ) || (previous.eq_ignore_ascii_case("on") && word.eq_ignore_ascii_case("stage"))
        {
            return true;
        }
        previous = word;
    }

    segments(title).any(|segment| {
        let Segment::Qualifier(inner) = segment else {
            return false;
        };
        let mut words = words(inner);
        let Some(first) = words.next() else {
            return false;
        };
        if is_any(first, &["live", "unplugged"]) {
            return true;
        }
        let mut previous = first;
        words.any(|word| {
            let found = word.eq_ignore_ascii_case("unplugged")
                || (previous.eq_ignore_ascii_case("in") && word.eq_ignore_ascii_case("concert"));
            previous = word;
            found
        })
    })
}

fn is_any(word: &str, candidates: &[&str]) -> bool {
    candidates
        .iter()
        .any(|candidate| word.eq_ignore_ascii_case(candidate))
}

/// Runs of word characters (`[A-Za-z0-9_]`, like the regex class `\w`).
fn words(text: &str) -> impl Iterator<Item = &str> {
    text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .filter(|word| !word.is_empty())
}

/// A piece of a title: plain text, or the inside of a closed `(…)` / `[…]` qualifier.
#[derive(Debug, PartialEq, Eq)]
enum Segment<'a> {
    Text(&'a str),
    Qualifier(&'a str),
}

/// Splits a title into text and qualifiers. An unclosed bracket is plain text.
fn segments(title: &str) -> impl Iterator<Item = Segment<'_>> {
    let mut rest = title;
    std::iter::from_fn(move || {
        if rest.is_empty() {
            return None;
        }
        // Find the next bracket that has a matching closing bracket.
        let mut search_from = 0;
        while let Some(offset) = rest[search_from..].find(['(', '[']) {
            let open = search_from + offset;
            let close = if rest.as_bytes()[open] == b'(' {
                ')'
            } else {
                ']'
            };
            if let Some(length) = rest[open + 1..].find(close) {
                if open > 0 {
                    let text = &rest[..open];
                    rest = &rest[open..];
                    return Some(Segment::Text(text));
                }
                let inner = &rest[1..=length];
                rest = &rest[length + 2..];
                return Some(Segment::Qualifier(inner));
            }
            search_from = open + 1;
        }
        let text = rest;
        rest = "";
        Some(Segment::Text(text))
    })
}

fn chronological(a: &Album, b: &Album) -> Ordering {
    a.release_date.cmp(&b.release_date).then_with(|| {
        let a = a.title.bytes().map(|c| c.to_ascii_lowercase());
        let b = b.title.bytes().map(|c| c.to_ascii_lowercase());
        a.cmp(b)
    })
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    fn image(url: &str, size: Option<u32>) -> Image {
        Image {
            url: url.to_owned(),
            width: size,
            height: size,
        }
    }

    fn release(title: &str, date: &str, release_type: ReleaseType) -> Release {
        Release {
            id: title.to_owned(),
            title: title.to_owned(),
            release_date: date.to_owned(),
            release_type,
            url: format!("https://example.com/{title}"),
            images: vec![
                image("small", Some(64)),
                image(&format!("{title}.jpg"), Some(640)),
            ],
            formats: Formats::default(),
            by_artist: true,
        }
    }

    #[test]
    fn best_cover_prefers_the_largest_image() {
        let images = [
            image("small", Some(64)),
            image("large", Some(1280)),
            image("medium", Some(640)),
            image("unknown", None),
        ];
        assert_eq!(best_cover(&images).unwrap().url, "large");
    }

    #[test]
    fn best_cover_keeps_the_first_image_on_ties_and_unknown_sizes() {
        assert_eq!(
            best_cover(&[image("a", None), image("b", None)])
                .unwrap()
                .url,
            "a"
        );
        assert_eq!(
            best_cover(&[image("a", Some(640)), image("b", Some(640))])
                .unwrap()
                .url,
            "a"
        );
        assert_eq!(best_cover(&[]), None);
    }

    #[test]
    fn normalize_title_strips_qualifiers_dashes_and_extra_whitespace() {
        assert_eq!(normalize_title("First Album (Deluxe)"), "first album");
        assert_eq!(
            normalize_title("First Album [Remastered 2011]"),
            "first album"
        );
        assert_eq!(normalize_title("A \u{2013} B"), "a - b");
        assert_eq!(normalize_title("A\u{2014}B (x)"), "a-b");
        assert_eq!(normalize_title("  A \t\n B  "), "a b");
        assert_eq!(normalize_title("Songs (Unclosed"), "songs (unclosed");
        assert_eq!(normalize_title("Björk"), "björk");
    }

    #[test]
    fn segments_split_text_and_closed_qualifiers() {
        let parts: Vec<_> = segments("A (b) [c] (d").collect();
        assert_eq!(
            parts,
            [
                Segment::Text("A "),
                Segment::Qualifier("b"),
                Segment::Text(" "),
                Segment::Qualifier("c"),
                Segment::Text(" (d")
            ]
        );
    }

    #[test]
    fn recognizes_common_live_album_titles() {
        for title in [
            "Live at Wembley",
            "Alive (Live)",
            "Greatest Hits [Live]",
            "Hits (Live from the Royal Albert Hall)",
            "MTV Unplugged",
            "Unplugged in New York",
            "Classics (MTV Unplugged)",
            "In Concert",
            "Hits (In Concert)",
            "On Stage",
            "The BBC Sessions",
            "Peel Session",
            "LIVE IN TOKYO",
        ] {
            assert!(is_likely_live(title), "expected live: {title:?}");
        }
    }

    #[test]
    fn leaves_studio_album_titles_alone() {
        for title in [
            "Abbey Road",
            "Alive",
            "Piano Concerto No. 1",
            "Hits (Deluxe Edition with Live Bonus Tracks)",
            "Delivery",
            "Stage Fright",
            "Back on the Stage Door", // "on" and "stage" are not adjacent
            "Songs (Remastered 2011)",
        ] {
            assert!(!is_likely_live(title), "expected studio: {title:?}");
        }
    }

    #[test]
    fn live_detection_applies_to_albums_only() {
        assert_eq!(
            categorize(&release("Live in Paris", "2000", ReleaseType::Album)),
            Category::LiveAlbum
        );
        assert_eq!(
            categorize(&release("Paris", "2000", ReleaseType::Album)),
            Category::StudioAlbum
        );
        assert_eq!(
            categorize(&release("Live in Paris", "2000", ReleaseType::Ep)),
            Category::Ep
        );
        assert_eq!(
            categorize(&release("Live in Paris", "2000", ReleaseType::Single)),
            Category::Single
        );
        assert_eq!(
            categorize(&release("Live", "2000", ReleaseType::Compilation)),
            Category::Compilation
        );
    }

    #[test]
    fn select_filters_deduplicates_and_sorts() {
        let mut appears_on = release("Various Hits", "2016", ReleaseType::Compilation);
        appears_on.by_artist = false;
        let mut no_cover = release("Bare", "2017", ReleaseType::Album);
        no_cover.images.clear();

        let releases = vec![
            release(
                "Night Drive (Deluxe Edition)",
                "2021-03-05",
                ReleaseType::Album,
            ),
            release("Night Drive", "2021-03-05", ReleaseType::Album), // duplicate of the deluxe edition
            release("Night Drive", "2023-01-01", ReleaseType::Album), // a re-release on another date stays
            release("Live at the Roundhouse", "2019-11", ReleaseType::Album),
            release("first light", "2015", ReleaseType::Album),
            release("Echoes", "2015", ReleaseType::Album),
            release("Signals", "2018", ReleaseType::Ep),
            appears_on,
            no_cover,
        ];

        let albums = select(releases, Include::ALBUMS);
        let summary: Vec<_> = albums
            .iter()
            .map(|a| (a.year(), a.title.as_str(), a.category))
            .collect();
        assert_eq!(
            summary,
            [
                ("2015", "Echoes", Category::StudioAlbum),
                ("2015", "first light", Category::StudioAlbum),
                ("2019", "Live at the Roundhouse", Category::LiveAlbum),
                (
                    "2021",
                    "Night Drive (Deluxe Edition)",
                    Category::StudioAlbum
                ),
                ("2023", "Night Drive", Category::StudioAlbum),
            ]
        );
        assert_eq!(albums[0].cover.url, "Echoes.jpg");
    }

    #[test]
    fn select_keeps_the_copy_with_the_best_audio() {
        let lossless = Formats {
            lossless: true,
            ..Formats::default()
        };
        let hires = Formats {
            hires: true,
            ..lossless
        };
        let atmos = Formats {
            atmos: true,
            ..Formats::default()
        };
        let copy = |id: u32, formats| {
            let mut release = release("Signs", "2019-02-15", ReleaseType::Album);
            release.url = format!("https://tidal.com/album/{id}/u");
            release.formats = formats;
            release
        };

        let albums = select(
            vec![
                copy(1, lossless),
                copy(2, atmos),
                copy(3, hires),
                copy(4, hires),
            ],
            Include::ALBUMS,
        );
        assert_eq!(albums.len(), 1);
        assert_eq!(albums[0].url, "https://tidal.com/album/3/u"); // the first hi-res copy
        assert_eq!(albums[0].badges, [Badge::Hires]);

        // Stereo beats an Atmos-only copy.
        let albums = select(vec![copy(1, atmos), copy(2, lossless)], Include::ALBUMS);
        assert_eq!(albums[0].url, "https://tidal.com/album/2/u");
    }

    #[test]
    fn badges_show_the_best_stereo_quality_and_atmos() {
        let badges = |lossless, hires, atmos| {
            Badge::for_formats(Formats {
                lossless,
                hires,
                atmos,
            })
        };
        assert_eq!(badges(false, false, false), Vec::<Badge>::new());
        assert_eq!(badges(true, false, false), [Badge::Lossless]);
        assert_eq!(badges(true, true, false), [Badge::Hires]);
        assert_eq!(badges(false, false, true), [Badge::Atmos]);
        assert_eq!(badges(true, true, true), [Badge::Hires, Badge::Atmos]);
    }

    #[test]
    fn select_keeps_requested_extras() {
        let releases = vec![
            release("Signals", "2018", ReleaseType::Ep),
            release("Hello", "2014", ReleaseType::Single),
        ];
        let include = Include {
            eps: true,
            ..Include::ALBUMS
        };
        let titles: Vec<_> = select(releases, include)
            .into_iter()
            .map(|a| a.title)
            .collect();
        assert_eq!(titles, ["Signals"]);
    }

    #[test]
    fn category_counts_are_pluralized() {
        assert_eq!(Category::StudioAlbum.count(1), "1 studio album");
        assert_eq!(Category::Ep.count(2), "2 EPs");
    }
}
