# What this project relies on, per crate

When a crate gets a major upgrade, compare its changelog against the entries
below: these are the APIs and behaviors the code depends on, some of them
only implicitly. "Guarded by" names the tests that fail if the behavior changes;
"check by hand" marks behavior no offline test covers.

## Runtime dependencies (they ship in the binary)

### ureq (`src/http.rs`)
- `Agent::config_builder()` with `.http_status_as_error(false)`: 4xx/5xx must come back
  as responses, because retries read `Retry-After` and errors quote the response body.
  If the default flips or the option is renamed, retries and error messages break.
- `.timeout_global(Some(30s))`, `.user_agent(...)`, `.build().into()` for the `Agent`.
- `agent.get(url).header(k, v).call()` and `agent.post(url).header(..).send_form(pairs)`;
  responses are `http::Response<ureq::Body>`, read with `body_mut().read_to_string()`
  (default size limit 10 MB; TIDAL pages are ~50 KB).
- Default features: rustls + ring + webpki-roots + gzip. A change of TLS backend or root
  certificates can break HTTPS on some systems.
- Guarded by: `http::tests` (retries, Retry-After, error bodies, network errors), provider
  tests. Check by hand: a live run (`cargo test --test live -- --ignored tidal`), because the
  mock server is plain HTTP and never exercises TLS.

### clap (`src/cli.rs`)
- Derive API: `Parser`, `ValueEnum` with `#[value(alias = ...)]`, `value_delimiter = ','`,
  `value_parser = fn`, `conflicts_with`, `default_value`, `after_help`.
- `clap::ColorChoice` implementing `ValueEnum` (used directly for `--color`).
- `ArtistLink` is parsed via `FromStr`; invalid links must stay usage errors (exit code 2).
- Guarded by: `cli::tests` (incl. `Cli::command().debug_assert()`), `tests/cli.rs`
  (help text fragments, exit codes, messages). Help wording changes are cosmetic, but make
  sure the README's options table still matches `covers --help`.

### dotenvy (`src/credentials.rs`)
- `from_path_iter(path)` must only *read* the file. It must never set process environment
  variables, or file values could start winning over real environment variables.
- A missing file is `Error::Io` with `NotFound` and is treated as "no variables".
- `Error::LineParse(line, index)` gives the offending line's text. The code uses it only to
  find the line *number*, so secrets never appear in messages.
- Guarded by: `credentials::tests` (`reads_env_files_and_ignores_missing_ones`,
  `malformed_files_report_the_line_number_but_not_its_content`), `tests/cli.rs`
  (`malformed_credentials_files_are_reported`).

### etcetera (`src/credentials.rs`)
- `choose_base_strategy()?.config_dir()` must keep resolving to `~/.config` (or
  `$XDG_CONFIG_HOME`) on macOS and Linux and `%APPDATA%` on Windows. Users' credentials
  live there; if a new version switched macOS to `~/Library/Application Support`, the
  tool would silently stop finding them.
- Check by hand (no test covers the real path): run `covers` without credentials from a
  directory without `.env` and read the path the error message suggests.

### indicatif (`src/progress.rs`)
- `ProgressBar::new_spinner()` draws on stderr and is hidden when stderr is not a terminal;
  `is_hidden()` decides between redrawing the spinner and printing plain wait lines.
- `ProgressStyle::with_template("{spinner:.dim} {msg:.dim}")`, `set_message`,
  `enable_steady_tick`, `finish_and_clear`.
- Check by hand: `script -q /dev/null target/debug/covers <link>` shows the spinner and
  that it's cleared before the summary.

### anstream / anstyle (`src/output.rs`, `src/main.rs`)
- `anstream::stdout()/stderr()` strip styles when not a terminal or with `NO_COLOR`;
  `ColorChoice::write_global()` applies `--color`.
- `anstyle::Style` constants rendered with `{STYLE}` / `{STYLE:#}`; tests strip with
  `anstream::adapter::strip_str`.
- Guarded by: `output::tests` (`text_is_styled` pins the exact escape codes).

### thiserror (`src/error.rs`, `src/link.rs`)
- `#[error("…", expr = .field.method())]` format arguments that read fields.
- Guarded by: every test that compares an error message.

### serde / serde_json (`src/providers/*`, `src/output.rs`)
- `src/providers/tidal.rs` decodes `included` with an internally tagged enum
  (`#[serde(tag = "type")]`) whose `#[serde(other)] Other` variant absorbs unknown types.
- Unknown fields are ignored (the serde default); missing optional fields use `#[serde(default)]`.
- JSON output field names and order are a public format (`output::tests::json_is_a_stable_document`).

### base64 (`src/providers/mod.rs`)
- `base64::engine::general_purpose::STANDARD.encode(...)` for the `Basic` auth header.
- Guarded by: `providers::tests::token_request_uses_basic_auth_and_a_form_body`.

## Test-only dependencies (they never ship)

### mockito
- `mockito::Server::new()` (synchronous), `server.url()`, `mock(method, path)` where a
  `&str` path matches the request's path *and query* exactly.
- When several mocks match, the first one that still expects hits wins, otherwise the last
  match. The retry tests depend on this: a 429 mock followed by a 200 mock on the same path.
- Unmatched requests get HTTP 501. The client retries 5xx, so a missing mock shows up
  as a few retries and then "answered HTTP 501", not as a mockito error.
- Dropping a `Mock` handle does not remove the mock.

### assert_cmd / predicates
- `assert_cmd::cargo::cargo_bin_cmd!("covers")` (uses `CARGO_BIN_EXE_covers`).

### tempfile, indoc, pretty_assertions
- Low risk; fix compile errors as they come.
