//! Whole runs (credentials → provider → selection → output) against a local
//! mock server that replays recorded API responses.

use std::cell::RefCell;
use std::time::Duration;

use album_covers::catalog::Service;
use album_covers::cli::Cli;
use album_covers::credentials::Sources;
use album_covers::http::{RetryObserver, RetryPolicy, Wait};
use album_covers::progress::Progress;
use album_covers::providers::Endpoints;
use album_covers::{Config, run};
use clap::Parser;
use indoc::indoc;
use mockito::{Mock, ServerGuard};
use pretty_assertions::assert_eq;

const SPOTIFY_LINK: &str = "https://open.spotify.com/artist/0TestPilots00000000000?si=share";
const TIDAL_LINK: &str = "https://tidal.com/artist/4242/u";

fn fixture(path: &str, server: &ServerGuard) -> String {
    let path = format!("{}/tests/fixtures/{path}", env!("CARGO_MANIFEST_DIR"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|err| panic!("{path}: {err}"));
    // Spotify's pagination links are absolute; point them at the test server.
    text.replace(
        "https://api.spotify.com/v1",
        &format!("{}/spotify/v1", server.url()),
    )
}

fn config(server: &ServerGuard) -> Config {
    let credentials = ["SPOTIFY", "TIDAL"].into_iter().flat_map(|prefix| {
        [
            (format!("{prefix}_CLIENT_ID"), "id".to_owned()),
            (format!("{prefix}_CLIENT_SECRET"), "secret".to_owned()),
        ]
    });
    Config {
        credentials: Sources::default().with_vars(credentials),
        spotify: Endpoints {
            token_url: format!("{}/spotify/api/token", server.url()),
            api_base: format!("{}/spotify/v1", server.url()),
        },
        tidal: Endpoints {
            token_url: format!("{}/tidal/token", server.url()),
            api_base: format!("{}/tidal/v2", server.url()),
        },
        retry: RetryPolicy {
            backoff: Duration::ZERO,
            ..RetryPolicy::default()
        },
    }
}

/// Records progress notifications.
#[derive(Default)]
struct Events(RefCell<Vec<String>>);

impl RetryObserver for Events {
    fn waiting(&self, wait: Wait) {
        self.0
            .borrow_mut()
            .push(format!("waiting after HTTP {}", wait.status));
    }
}

impl Progress for Events {
    fn start(&self, service: Service) {
        self.0.borrow_mut().push(format!("start {service}"));
    }
    fn finish(&self) {
        self.0.borrow_mut().push("finish".to_owned());
    }
}

struct Output {
    stdout: String,
    stderr: String,
    events: Vec<String>,
}

fn covers(server: &ServerGuard, args: &[&str]) -> Result<Output, album_covers::Error> {
    let cli = Cli::try_parse_from(std::iter::once("covers").chain(args.iter().copied())).unwrap();
    let (mut stdout, mut stderr, events) = (Vec::new(), Vec::new(), Events::default());
    run(&cli, &config(server), &mut stdout, &mut stderr, &events)?;
    let plain = |bytes: Vec<u8>| {
        anstream::adapter::strip_str(&String::from_utf8(bytes).unwrap()).to_string()
    };
    Ok(Output {
        stdout: plain(stdout),
        stderr: plain(stderr),
        events: events.0.into_inner(),
    })
}

fn mock_spotify(server: &mut ServerGuard, include_groups: &str) -> Vec<Mock> {
    let artist = "/spotify/v1/artists/0TestPilots00000000000";
    let bodies = [
        "token.json",
        "artist.json",
        "albums-page-1.json",
        "albums-page-2.json",
    ]
    .map(|name| fixture(&format!("spotify/{name}"), server));
    let [token, artist_body, page_1, page_2] = bodies;
    vec![
        server
            .mock("POST", "/spotify/api/token")
            .with_body(token)
            .create(),
        server.mock("GET", artist).with_body(artist_body).create(),
        server
            .mock(
                "GET",
                format!("{artist}/albums?include_groups={include_groups}&market=US&limit=10")
                    .as_str(),
            )
            .with_body(page_1)
            .create(),
        server
            .mock(
                "GET",
                format!("{artist}/albums?offset=10&limit=10&include_groups=album&market=US")
                    .as_str(),
            )
            .with_body(page_2)
            .create(),
    ]
}

#[test]
fn text_output_puts_the_summary_on_stderr_and_the_albums_on_stdout() {
    let mut server = mockito::Server::new();
    let mocks = mock_spotify(&mut server, "album");

    let output = covers(&server, &[SPOTIFY_LINK]).unwrap();

    mocks.iter().for_each(Mock::assert);
    assert_eq!(
        output.stderr,
        "The Test Pilots on Spotify (US): 2 studio albums, 1 live album\n\n"
    );
    assert_eq!(
        output.stdout,
        indoc! {"
            Studio albums (2)

              2015  First Light
                    album  https://open.spotify.com/album/1FirstLight00000000000
                    cover  https://i.scdn.co/image/ab67616d0000b273firstlight
              2021  Night Drive (Deluxe Edition)
                    album  https://open.spotify.com/album/1NightDriveDeluxe00000
                    cover  https://i.scdn.co/image/ab67616d0000b273nightdrivedeluxe

            Live albums (1)

              2019  Live at the Roundhouse
                    album  https://open.spotify.com/album/1LiveRoundhouse0000000
                    cover  https://i.scdn.co/image/ab67616d0000b273roundhouse
        "}
    );
    assert_eq!(output.events, ["start Spotify", "finish"]);
}

#[test]
fn include_all_adds_eps_and_singles_but_not_releases_by_others() {
    let mut server = mockito::Server::new();
    let mocks = mock_spotify(&mut server, "album,single,compilation");

    let output = covers(&server, &["--quiet", "--include", "all", SPOTIFY_LINK]).unwrap();

    mocks.iter().for_each(Mock::assert);
    assert_eq!(output.stderr, "");
    assert_eq!(output.events, Vec::<String>::new()); // --quiet silences progress too
    assert!(
        output.stdout.contains("EPs (1)\n\n  2018  Signals\n"),
        "{}",
        output.stdout
    );
    assert!(
        output.stdout.contains("Singles (1)\n\n  2014  Hello\n"),
        "{}",
        output.stdout
    );
    assert!(
        !output.stdout.contains("Indie Sampler"),
        "{}",
        output.stdout
    );
}

#[test]
fn json_output_is_one_document_on_stdout() {
    let mut server = mockito::Server::new();
    let _mocks = mock_spotify(&mut server, "album");

    let output = covers(&server, &["--json", SPOTIFY_LINK]).unwrap();

    assert_eq!(output.stderr, "");
    let document: serde_json::Value = serde_json::from_str(&output.stdout).unwrap();
    assert_eq!(document["provider"], "spotify");
    assert_eq!(document["country"], "US");
    assert_eq!(document["artist"]["name"], "The Test Pilots");
    assert_eq!(document["albums"].as_array().unwrap().len(), 3);
    assert_eq!(document["albums"][0]["category"], "studio_album");
    assert_eq!(document["albums"][0]["cover"]["width"], 640);
    // Spotify doesn't say which audio formats an album has.
    assert_eq!(document["albums"][0]["badges"], serde_json::json!([]));
}

#[test]
fn tidal_end_to_end_with_a_rate_limited_page() {
    let mut server = mockito::Server::new();
    let page_2 = "/tidal/v2/artists/4242/relationships/albums?countryCode=US&page%5Bcursor%5D=page2&include=albums.coverArt";
    let mocks = [
        server
            .mock("POST", "/tidal/token")
            .with_body(fixture("tidal/token.json", &server))
            .create(),
        server
            .mock(
                "GET",
                "/tidal/v2/artists/4242?countryCode=US&include=albums.coverArt",
            )
            .with_body(fixture("tidal/artist.json", &server))
            .create(),
        server
            .mock("GET", page_2)
            .with_status(429)
            .with_header("retry-after", "0")
            .create(),
        server
            .mock("GET", page_2)
            .with_body(fixture("tidal/albums-page-2.json", &server))
            .create(),
    ];

    let output = covers(&server, &[TIDAL_LINK]).unwrap();

    mocks.iter().for_each(Mock::assert);
    assert_eq!(
        output.events,
        ["start TIDAL", "waiting after HTTP 429", "finish"]
    );
    assert_eq!(
        output.stderr,
        "The Test Pilots on TIDAL (US): 2 studio albums, 1 live album\n\n"
    );
    assert_eq!(
        output.stdout,
        indoc! {"
            Studio albums (2)

              2015  First Light  [Hi-Res]
                    album  https://tidal.com/album/1006/u
                    cover  https://resources.tidal.com/images/first/light/1280x1280.jpg
              2021  Night Drive (Deluxe Edition)  [Hi-Res]
                    album  https://tidal.com/album/1002/u
                    cover  https://resources.tidal.com/images/night/drive/1280x1280.jpg

            Live albums (1)

              2019  Live at the Roundhouse  [Lossless] [Atmos]
                    album  https://tidal.com/album/1003/u
                    cover  https://resources.tidal.com/images/roundhouse/1280x1280.jpg
        "}
    );
}

#[test]
fn rejected_credentials_quote_the_services_explanation() {
    let mut server = mockito::Server::new();
    server
        .mock("POST", "/spotify/api/token")
        .with_status(400)
        .with_body(r#"{"error":"invalid_client","error_description":"Invalid client secret"}"#)
        .create();

    let err = covers(&server, &[SPOTIFY_LINK]).err().unwrap();

    assert_eq!(
        err.to_string(),
        "Spotify did not accept the client credentials: Invalid client secret\n\
         Check SPOTIFY_CLIENT_ID and SPOTIFY_CLIENT_SECRET."
    );
}
