//! Checks against the real services. They need credentials (environment,
//! `./.env` or the user's credentials file) and network access, so they only
//! run on request:
//!
//! ```sh
//! cargo test --test live -- --ignored
//! cargo test --test live -- --ignored tidal   # just one service
//! ```

use album_covers::cli::Cli;
use album_covers::{Config, run};
use clap::Parser;

fn covers(link: &str) -> String {
    let cli = Cli::try_parse_from(["covers", "--quiet", "--include", "all", link]).unwrap();
    let config = Config::standard().unwrap();
    let mut stdout = Vec::new();
    run(&cli, &config, &mut stdout, &mut Vec::new(), &()).unwrap_or_else(|err| panic!("{err}"));
    String::from_utf8(stdout).unwrap()
}

#[test]
#[ignore = "calls the real TIDAL API"]
fn tidal() {
    // Tedeschi Trucks Band
    let output = covers("https://tidal.com/artist/3974492/u");
    assert!(output.contains("Revelator"), "{output}");
    assert!(
        output.contains("https://resources.tidal.com/images/"),
        "{output}"
    );
    // Most of the band's albums are on TIDAL in hi-res.
    assert!(output.contains("[Hi-Res]"), "{output}");
}

#[test]
#[ignore = "calls the real Spotify API (the app owner needs Spotify Premium)"]
fn spotify() {
    // Radiohead
    let output = covers("https://open.spotify.com/artist/4Z8W4fKeB5YxbusRsdQVPb");
    assert!(output.contains("OK Computer"), "{output}");
    assert!(output.contains("https://i.scdn.co/image/"), "{output}");
}
