//! The real `covers` binary: arguments, exit codes and messages.
//! Nothing here touches the network.

use assert_cmd::Command;
use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;

/// `covers` in an empty directory, with no credentials anywhere.
fn covers(home: &tempfile::TempDir) -> Command {
    let mut cmd = cargo_bin_cmd!("covers");
    cmd.env_clear()
        .current_dir(home.path())
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", home.path().join(".config"))
        .env("USERPROFILE", home.path())
        .env("APPDATA", home.path().join("AppData"));
    cmd
}

#[test]
fn help_shows_usage_examples_and_credentials() {
    let home = tempfile::tempdir().unwrap();
    covers(&home)
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Usage: covers [OPTIONS] <ARTIST_LINK>",
        ))
        .stdout(predicate::str::contains("Examples:"))
        .stdout(predicate::str::contains("TIDAL_CLIENT_ID"));
}

#[test]
fn version() {
    let home = tempfile::tempdir().unwrap();
    covers(&home)
        .arg("--version")
        .assert()
        .success()
        .stdout(format!("covers {}\n", env!("CARGO_PKG_VERSION")));
}

#[test]
fn a_missing_link_is_a_usage_error() {
    let home = tempfile::tempdir().unwrap();
    covers(&home)
        .assert()
        .code(2)
        .stderr(predicate::str::contains(
            "the following required arguments were not provided",
        ));
}

#[test]
fn links_to_other_pages_are_usage_errors() {
    let home = tempfile::tempdir().unwrap();
    covers(&home)
        .arg("https://open.spotify.com/album/5xlOpJPst87BgM2kNLXCTC")
        .assert()
        .code(2)
        .stderr(predicate::str::contains(
            "this link does not point to an artist page",
        ));
}

#[test]
fn bad_option_values_are_usage_errors() {
    let home = tempfile::tempdir().unwrap();
    covers(&home)
        .args(["--country", "USA", "https://tidal.com/artist/1"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains(
            "expected two letters, like US or DE",
        ));
    covers(&home)
        .args(["--include", "remixes", "https://tidal.com/artist/1"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains(
            "possible values: eps, singles, compilations, all",
        ));
}

#[test]
fn missing_credentials_say_what_to_set_and_where() {
    let home = tempfile::tempdir().unwrap();
    covers(&home)
        .arg("https://tidal.com/artist/3974492/u")
        .assert()
        .code(1)
        .stdout("")
        .stderr(predicate::str::starts_with(
            "error: missing TIDAL credentials: TIDAL_CLIENT_ID, TIDAL_CLIENT_SECRET",
        ))
        .stderr(predicate::str::contains(
            "https://developer.tidal.com/dashboard",
        ))
        .stderr(predicate::str::contains("credentials.env"));
}

#[test]
fn malformed_credentials_files_are_reported() {
    let home = tempfile::tempdir().unwrap();
    std::fs::write(
        home.path().join(".env"),
        "TIDAL_CLIENT_ID=abc\nnot a variable\n",
    )
    .unwrap();
    covers(&home)
        .arg("https://tidal.com/artist/1")
        .assert()
        .code(1)
        .stderr(predicate::str::contains(
            ".env: line 2 is not in KEY=value form",
        ));
}
