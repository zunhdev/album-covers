//! The command line.

use clap::{ColorChoice, Parser, ValueEnum};

use crate::catalog::Include;
use crate::link::ArtistLink;

const AFTER_HELP: &str = "\
Examples:
  covers https://tidal.com/artist/3974492
  covers --include eps,singles https://open.spotify.com/artist/4Z8W4fKeB5YxbusRsdQVPb
  covers --json https://tidal.com/artist/3974492 | jq -r '.albums[].cover.url'

Credentials:
  TIDAL needs TIDAL_CLIENT_ID and TIDAL_CLIENT_SECRET; Spotify needs
  SPOTIFY_CLIENT_ID and SPOTIFY_CLIENT_SECRET. Each is read from the
  environment, ./.env, or ~/.config/album-covers/credentials.env.";

/// List an artist's albums with links to the albums and their cover art.
#[derive(Debug, Parser)]
#[command(name = "covers", version, after_help = AFTER_HELP)]
pub struct Cli {
    /// Link to an artist on Spotify or TIDAL (see the examples below)
    #[arg(value_name = "ARTIST_LINK")]
    pub link: ArtistLink,

    /// Also list other release types (comma-separated)
    #[arg(short, long, value_name = "TYPES", value_delimiter = ',')]
    pub include: Vec<Extra>,

    /// Catalog country (ISO 3166-1 alpha-2); availability differs by country
    #[arg(short, long, value_name = "CODE", default_value = "US", value_parser = parse_country)]
    pub country: String,

    /// Output format
    #[arg(short, long, value_name = "FORMAT", default_value = "text")]
    pub format: Format,

    /// Same as --format json
    #[arg(long, conflicts_with = "format")]
    pub json: bool,

    /// Print only the albums, without progress or summary on stderr
    #[arg(short, long)]
    pub quiet: bool,

    /// When to use colors
    #[arg(long, value_name = "WHEN", default_value = "auto")]
    pub color: ColorChoice,
}

/// Release types that can be listed besides albums.
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum Extra {
    #[value(alias = "ep")]
    Eps,
    #[value(alias = "single")]
    Singles,
    /// Spotify only
    #[value(alias = "compilation")]
    Compilations,
    /// All of the above
    All,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum Format {
    /// Grouped list for reading
    Text,
    /// One JSON document for scripts
    Json,
}

impl Cli {
    /// The release types to list.
    #[must_use]
    pub fn include(&self) -> Include {
        self.include
            .iter()
            .fold(Include::ALBUMS, |include, extra| match extra {
                Extra::Eps => Include {
                    eps: true,
                    ..include
                },
                Extra::Singles => Include {
                    singles: true,
                    ..include
                },
                Extra::Compilations => Include {
                    compilations: true,
                    ..include
                },
                Extra::All => Include::ALL,
            })
    }

    /// The output format, taking `--json` into account.
    #[must_use]
    pub fn output_format(&self) -> Format {
        if self.json { Format::Json } else { self.format }
    }

    /// The color setting for [`anstream`].
    #[must_use]
    pub fn color_choice(&self) -> anstream::ColorChoice {
        match self.color {
            ColorChoice::Auto => anstream::ColorChoice::Auto,
            ColorChoice::Always => anstream::ColorChoice::Always,
            ColorChoice::Never => anstream::ColorChoice::Never,
        }
    }
}

fn parse_country(value: &str) -> Result<String, String> {
    if value.len() == 2 && value.bytes().all(|c| c.is_ascii_alphabetic()) {
        Ok(value.to_ascii_uppercase())
    } else {
        Err("expected two letters, like US or DE".to_owned())
    }
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory;
    use clap::error::ErrorKind;

    use super::*;
    use crate::catalog::Service;

    fn parse(args: &[&str]) -> Result<Cli, clap::Error> {
        Cli::try_parse_from(std::iter::once("covers").chain(args.iter().copied()))
    }

    #[test]
    fn the_command_is_well_formed() {
        Cli::command().debug_assert();
    }

    #[test]
    fn a_bare_link_uses_the_defaults() {
        let cli = parse(&["https://tidal.com/artist/1"]).unwrap();
        assert_eq!(cli.link.service, Service::Tidal);
        assert_eq!(cli.link.id, "1");
        assert_eq!(cli.include(), Include::ALBUMS);
        assert_eq!(cli.country, "US");
        assert_eq!(cli.output_format(), Format::Text);
        assert!(!cli.quiet);
    }

    #[test]
    fn options_in_both_spellings() {
        let cli = parse(&[
            "--include",
            "eps",
            "--include=single",
            "--country=de",
            "--format",
            "json",
            "--quiet",
            "spotify:artist:abc",
        ])
        .unwrap();
        assert_eq!(
            cli.include(),
            Include {
                eps: true,
                singles: true,
                compilations: false
            }
        );
        assert_eq!(cli.country, "DE");
        assert_eq!(cli.output_format(), Format::Json);
        assert!(cli.quiet);

        let cli = parse(&[
            "spotify:artist:abc",
            "-i",
            "eps,compilations",
            "-c",
            "gb",
            "-f",
            "json",
            "-q",
        ])
        .unwrap();
        assert_eq!(
            cli.include(),
            Include {
                eps: true,
                singles: false,
                compilations: true
            }
        );
        assert_eq!(cli.country, "GB");
    }

    #[test]
    fn include_all_and_json_shorthand() {
        let cli = parse(&["--json", "-i", "all", "spotify:artist:abc"]).unwrap();
        assert_eq!(cli.include(), Include::ALL);
        assert_eq!(cli.output_format(), Format::Json);
    }

    #[test]
    fn rejects_bad_input() {
        let kind = |args: &[&str]| parse(args).unwrap_err().kind();
        assert_eq!(kind(&[]), ErrorKind::MissingRequiredArgument);
        assert_eq!(
            kind(&["https://example.com/artist/1"]),
            ErrorKind::ValueValidation
        );
        assert_eq!(
            kind(&["-i", "remixes", "spotify:artist:abc"]),
            ErrorKind::InvalidValue
        );
        assert_eq!(
            kind(&["-c", "USA", "spotify:artist:abc"]),
            ErrorKind::ValueValidation
        );
        assert_eq!(
            kind(&["--json", "--format", "text", "spotify:artist:abc"]),
            ErrorKind::ArgumentConflict
        );
        assert_eq!(
            kind(&["spotify:artist:abc", "spotify:artist:def"]),
            ErrorKind::UnknownArgument
        );
    }

    #[test]
    fn link_errors_explain_what_is_wrong() {
        let message = parse(&["https://open.spotify.com/album/abc"])
            .unwrap_err()
            .to_string();
        assert!(
            message.contains("this link does not point to an artist page"),
            "{message}"
        );
    }
}
