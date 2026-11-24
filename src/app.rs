//! Ties the pieces together: credentials → provider → selection → output.

use std::io::Write;

use crate::albums;
use crate::catalog::Service;
use crate::cli::{Cli, Format};
use crate::credentials::Sources;
use crate::error::Result;
use crate::http::{Client, RetryPolicy};
use crate::output::{self, Report};
use crate::progress::Progress;
use crate::providers::{Endpoints, Query, spotify, tidal};

/// Everything a run needs from its environment.
#[derive(Clone, Debug)]
pub struct Config {
    pub credentials: Sources,
    pub spotify: Endpoints,
    pub tidal: Endpoints,
    pub retry: RetryPolicy,
}

impl Config {
    /// The real services, with credentials from the standard places.
    pub fn standard() -> Result<Self> {
        Ok(Self {
            credentials: Sources::standard()?,
            spotify: Endpoints::spotify(),
            tidal: Endpoints::tidal(),
            retry: RetryPolicy::default(),
        })
    }
}

/// Lists the albums of `cli.link` on `out`; the summary and progress go to `err` and `progress`.
pub fn run(
    cli: &Cli,
    config: &Config,
    out: &mut dyn Write,
    err: &mut dyn Write,
    progress: &dyn Progress,
) -> Result<()> {
    let service = cli.link.service;
    let credentials = config.credentials.credentials(service)?;
    let progress = if cli.quiet { &() } else { progress };
    let client = Client::new(service, config.retry, progress);
    let include = cli.include();
    let query = Query {
        artist_id: &cli.link.id,
        country: &cli.country,
        include,
    };

    progress.start(service);
    let catalog = match service {
        Service::Spotify => spotify::fetch_catalog(&client, &config.spotify, &credentials, query),
        Service::Tidal => tidal::fetch_catalog(&client, &config.tidal, &credentials, query),
    };
    progress.finish();
    let catalog = catalog?;

    let report = Report {
        service,
        country: cli.country.clone(),
        artist: catalog.artist,
        include,
        albums: albums::select(catalog.releases, include),
    };
    match cli.output_format() {
        Format::Text => {
            if !cli.quiet {
                output::write_summary(err, &report)?;
                if !report.albums.is_empty() {
                    writeln!(err)?;
                }
                err.flush()?;
            }
            output::write_text(out, &report)?;
        }
        Format::Json => output::write_json(out, &report)?,
    }
    out.flush()?;
    Ok(())
}
