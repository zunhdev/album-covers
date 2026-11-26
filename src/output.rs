//! Text and JSON output.
//!
//! Text output is split across the standard streams so that piping stays useful:
//! a one-line summary goes to stderr ([`write_summary`]) and the albums to
//! stdout ([`write_text`]). JSON output ([`write_json`]) is a single document on stdout.
//!
//! Styles are always written; the caller's stream decides whether to keep them
//! (see [`anstream`]), so colors disappear when piping or with `NO_COLOR`.

use std::io::{self, Write};

use anstyle::{AnsiColor, Style};
use serde::Serialize;

use crate::albums::{Album, Category};
use crate::catalog::{Artist, Include, Service};

const HEADING: Style = Style::new().bold();
const DIM: Style = Style::new().dimmed();
const YEAR: Style = AnsiColor::Cyan.on_default();
const BADGE: Style = AnsiColor::Magenta.on_default();

/// Everything the renderers need to know about a finished run.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Report {
    #[serde(rename = "provider")]
    pub service: Service,
    pub country: String,
    pub artist: Artist,
    /// Release types that were asked for (used for hints when nothing was found).
    #[serde(skip)]
    pub include: Include,
    /// Selected albums, sorted chronologically.
    pub albums: Vec<Album>,
}

impl Report {
    fn count(&self, category: Category) -> usize {
        self.albums
            .iter()
            .filter(|album| album.category == category)
            .count()
    }
}

/// Writes the albums grouped by category:
///
/// ```text
/// Studio albums (2)
///
///   2011  Revelator  [Hi-Res]
///         album  https://tidal.com/album/90411175/u
///         cover  https://resources.tidal.com/images/…/1280x1280.jpg
/// ```
pub fn write_text(w: &mut dyn Write, report: &Report) -> io::Result<()> {
    let mut first_group = true;
    for category in Category::ALL {
        let count = report.count(category);
        if count == 0 {
            continue;
        }
        if !first_group {
            writeln!(w)?;
        }
        first_group = false;

        writeln!(
            w,
            "{HEADING}{}{HEADING:#}{DIM} ({count}){DIM:#}\n",
            category.heading()
        )?;
        for album in report
            .albums
            .iter()
            .filter(|album| album.category == category)
        {
            write!(w, "  {YEAR}{:<4}{YEAR:#}  {}", album.year(), album.title)?;
            for (i, badge) in album.badges.iter().enumerate() {
                let gap = if i == 0 { "  " } else { " " };
                write!(w, "{gap}{BADGE}[{}]{BADGE:#}", badge.label())?;
            }
            writeln!(w)?;
            writeln!(w, "        {DIM}album{DIM:#}  {}", album.url)?;
            writeln!(w, "        {DIM}cover{DIM:#}  {}", album.cover.url)?;
        }
    }
    Ok(())
}

/// Writes a one-line summary such as
/// "Radiohead on TIDAL (US): 9 studio albums, 2 live albums",
/// or explains that nothing was found.
pub fn write_summary(w: &mut dyn Write, report: &Report) -> io::Result<()> {
    write!(
        w,
        "{HEADING}{}{HEADING:#} on {} ({})",
        report.artist.name, report.service, report.country
    )?;
    if report.albums.is_empty() {
        writeln!(w, ": no releases with cover art found.")?;
        if report.include != Include::ALL {
            writeln!(
                w,
                "{DIM}Hint: use --include all to list EPs, singles and compilations too.{DIM:#}"
            )?;
        }
        return Ok(());
    }
    let counts: Vec<String> = Category::ALL
        .into_iter()
        .map(|category| (category, report.count(category)))
        .filter(|&(_, count)| count > 0)
        .map(|(category, count)| category.count(count))
        .collect();
    writeln!(w, ": {}", counts.join(", "))
}

/// Writes the report as one pretty-printed JSON document.
pub fn write_json(w: &mut dyn Write, report: &Report) -> io::Result<()> {
    serde_json::to_writer_pretty(&mut *w, report)?;
    writeln!(w)
}

#[cfg(test)]
mod tests {
    use indoc::indoc;
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::albums::Badge;
    use crate::catalog::Image;

    fn report() -> Report {
        let album = |title: &str, date: &str, category, id: u32, size: Option<u32>| Album {
            title: title.to_owned(),
            release_date: date.to_owned(),
            category,
            badges: Vec::new(),
            url: format!("https://tidal.com/album/{id}/u"),
            cover: Image {
                url: format!("https://img.example/{id}.jpg"),
                width: size,
                height: size,
            },
        };
        let mut report = Report {
            service: Service::Tidal,
            country: "US".to_owned(),
            artist: Artist {
                id: "1".into(),
                name: "The Test Pilots".into(),
                url: "https://tidal.com/artist/1/u".into(),
            },
            include: Include::ALBUMS,
            albums: vec![
                album("First Light", "2015", Category::StudioAlbum, 10, Some(1280)),
                album(
                    "Live at the Roundhouse",
                    "2019-11",
                    Category::LiveAlbum,
                    20,
                    None,
                ),
                album(
                    "Night Drive",
                    "2021-03-05",
                    Category::StudioAlbum,
                    30,
                    Some(640),
                ),
            ],
        };
        report.albums[0].badges = vec![Badge::Hires];
        report.albums[2].badges = vec![Badge::Lossless, Badge::Atmos];
        report
    }

    fn render(write: fn(&mut dyn Write, &Report) -> io::Result<()>, report: &Report) -> String {
        let mut out = Vec::new();
        write(&mut out, report).unwrap();
        String::from_utf8(out).unwrap()
    }

    fn plain(text: &str) -> String {
        anstream::adapter::strip_str(text).to_string()
    }

    #[test]
    fn text_groups_albums_by_category() {
        assert_eq!(
            plain(&render(write_text, &report())),
            indoc! {"
                Studio albums (2)

                  2015  First Light  [Hi-Res]
                        album  https://tidal.com/album/10/u
                        cover  https://img.example/10.jpg
                  2021  Night Drive  [Lossless] [Atmos]
                        album  https://tidal.com/album/30/u
                        cover  https://img.example/30.jpg

                Live albums (1)

                  2019  Live at the Roundhouse
                        album  https://tidal.com/album/20/u
                        cover  https://img.example/20.jpg
            "}
        );
    }

    #[test]
    fn text_is_styled() {
        assert!(
            render(write_text, &report())
                .starts_with("\x1b[1mStudio albums\x1b[0m\x1b[2m (2)\x1b[0m\n")
        );
        assert!(render(write_text, &report()).contains("First Light  \x1b[35m[Hi-Res]\x1b[0m\n"));
    }

    #[test]
    fn summary_counts_each_category() {
        assert_eq!(
            plain(&render(write_summary, &report())),
            "The Test Pilots on TIDAL (US): 2 studio albums, 1 live album\n"
        );
    }

    #[test]
    fn summary_hints_at_include_when_nothing_was_found() {
        let mut report = report();
        report.albums.clear();
        assert_eq!(
            plain(&render(write_summary, &report)),
            indoc! {"
                The Test Pilots on TIDAL (US): no releases with cover art found.
                Hint: use --include all to list EPs, singles and compilations too.
            "}
        );

        report.include = Include::ALL;
        assert_eq!(
            plain(&render(write_summary, &report)),
            "The Test Pilots on TIDAL (US): no releases with cover art found.\n"
        );
    }

    #[test]
    fn json_is_a_stable_document() {
        let mut report = report();
        report.albums.truncate(2);
        assert_eq!(
            render(write_json, &report),
            indoc! {r#"
                {
                  "provider": "tidal",
                  "country": "US",
                  "artist": {
                    "id": "1",
                    "name": "The Test Pilots",
                    "url": "https://tidal.com/artist/1/u"
                  },
                  "albums": [
                    {
                      "title": "First Light",
                      "release_date": "2015",
                      "category": "studio_album",
                      "badges": [
                        "hires"
                      ],
                      "url": "https://tidal.com/album/10/u",
                      "cover": {
                        "url": "https://img.example/10.jpg",
                        "width": 1280,
                        "height": 1280
                      }
                    },
                    {
                      "title": "Live at the Roundhouse",
                      "release_date": "2019-11",
                      "category": "live_album",
                      "badges": [],
                      "url": "https://tidal.com/album/20/u",
                      "cover": {
                        "url": "https://img.example/20.jpg",
                        "width": null,
                        "height": null
                      }
                    }
                  ]
                }
            "#}
        );
    }
}
