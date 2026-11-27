# Contributing

## Setup

Install Rust with [rustup](https://rustup.rs). The toolchain version is pinned
in `rust-toolchain.toml`; rustup installs it automatically the first time you
run `cargo` in this folder. The oldest Rust that can build `covers` is the
`rust-version` in `Cargo.toml`, and CI checks it.

For the dependency checks, install [cargo-deny](https://github.com/EmbarkStudios/cargo-deny):
`cargo install --locked cargo-deny`.

## Everyday commands

```sh
cargo run -- https://tidal.com/artist/3974492    # run from source
cargo test                                       # all offline tests
cargo clippy --all-targets                       # lints (pedantic; CI denies warnings)
cargo fmt                                        # format
cargo deny check                                 # vulnerabilities, licenses, sources
cargo test --test live -- --ignored              # against the real APIs (needs credentials)
```

CI (`.github/workflows/ci.yml`) runs formatting, Clippy, docs, the tests on
Linux, macOS and Windows, a build with the minimum supported Rust, and cargo-deny.

## Releasing

Releases are built and published by `.github/workflows/release.yml` when a version tag is pushed.

1. In a pull request, bump `version` in `Cargo.toml` (and run `cargo check` so `Cargo.lock`
   follows). Merge it once CI is green.
2. Tag the merge commit on `main` and push the tag:
   ```sh
   git switch main && git pull
   git tag -a v0.2.0 -m "album-covers 0.2.0"
   git push origin v0.2.0
   ```
3. The workflow checks that the tag's release isn't published yet, that the tag matches
   `Cargo.toml`, that the tagged commit is on `main`, and that CI passed on it (it waits if CI
   is still running). It then builds `covers` for Linux
   (x86-64 and ARM64, static musl), macOS (Apple silicon and Intel) and Windows (x64), runs each
   binary once (the Intel macOS one under Rosetta), attests each archive's build provenance, and
   publishes the release with `SHA256SUMS` and notes generated from the merged pull requests.
4. To add your own notes, edit the release once it's published. The title and notes of an
   immutable release can still be changed.

Don't create or publish the release on GitHub's "Draft a new release" page. That page tags
whatever commit it targets, and a release published there is immutable at once: it has no
binaries, and its tag can't be moved or reused. While it is the Latest release, the README's
install links return 404. If that happens anyway, mark the previous release as Latest
(`gh release edit v0.1.1 --latest`), add a note to the empty release, and release the next
version.

A tag with a pre-release suffix, such as `v0.3.0-rc.1` (with `version = "0.3.0-rc.1"`), is
published as a pre-release. Only the newest stable version becomes the Latest release, so a fix
for an older line (`v0.1.1` after `v0.2.0`) doesn't replace it. If the publish job fails, re-run
it: it picks up the draft release from the failed attempt.

Pull requests that change the release workflow, `Cargo.toml`, `Cargo.lock` or
`rust-toolchain.toml` run the builds as a dry run, so an update that breaks a release target
shows up before the next tag. The dry run doesn't attest or publish.

Archive names contain no version (`album-covers-<target>.tar.gz`, or `.zip` for Windows), so
`releases/latest/download/…` links in the README always point at the newest release. The README
in each archive links to the other repository files on GitHub.
Only repository admins can create `v*` tags, and published releases are immutable.

## Keeping up to date

Dependabot opens weekly pull requests for in-range crate updates and the CI actions.
Major crate upgrades and new Rust versions are done by hand. Two Claude Code skills
describe the steps, and are readable as plain checklists too:

- `.claude/skills/update-dependencies/` covers crate updates and major upgrades.
  `scripts/outdated.sh` lists what's outdated; `references/crate-notes.md` lists what
  the code relies on in each crate.
- `.claude/skills/update-rust-toolchain/` covers the pinned toolchain
  (`rust-toolchain.toml`), the minimum supported Rust (`rust-version`) and the edition.

## How it works

```text
cli ──▶ credentials ──▶ providers::{spotify, tidal} ──▶ albums::select ──▶ output
         (env, .env,      (over http, with retries)       (filter, dedupe,    (text or
          config file)                                     classify, sort)     JSON)
```

| Module        | Responsibility                                                              |
| ------------- | --------------------------------------------------------------------------- |
| `main.rs`     | The binary: parses arguments, sets up colors and the spinner, prints errors. |
| `app.rs`      | `run`: one invocation, from credentials to output. `Config` holds everything that differs between production and tests. |
| `cli.rs`      | The command line (clap derive).                                              |
| `link.rs`     | Artist links → service and ID (`FromStr`, so clap validates links).          |
| `credentials.rs` | Credential lookup: environment, `./.env`, the per-user `credentials.env`. |
| `providers/`  | One module per service, turning its API into a `catalog::Catalog`. Shared OAuth client-credentials flow in `mod.rs`. |
| `http.rs`     | Blocking HTTP (ureq) with retries for 429/5xx and readable errors.           |
| `albums.rs`   | Pure selection logic: best cover, live-album detection, de-duplication (keeping the best-sounding copy), audio badges, sorting. |
| `output.rs`   | Text and JSON rendering.                                                     |
| `progress.rs` | The spinner, and retry notices on stderr.                                    |
| `error.rs`    | The `Error` enum. Every message is written for the end user.                 |

Provider notes (request flow, page sizes, rate limits, quirks) are in the
module docs of `providers/spotify.rs` and `providers/tidal.rs`.

## Tests

- **Unit tests** live next to the code (`#[cfg(test)]` modules).
- **Provider and HTTP tests** run against a local [mockito](https://docs.rs/mockito)
  server that replays recorded responses from `tests/fixtures/`. They check
  what is sent (paths, query strings, headers), not just what comes back.
- **`tests/app.rs`** runs whole invocations of `run` against the mock server.
- **`tests/cli.rs`** runs the real binary for arguments, exit codes and messages.
- **`tests/live.rs`** talks to the real services. Its tests are `#[ignore]`d
  so the normal test run never needs the network or credentials.

Fixtures are trimmed copies of real responses with a fictional artist
("The Test Pilots"). When an API changes, record a fresh response, trim it,
and keep the IDs consistent with the tests.

## Conventions

- Format with `cargo fmt`; keep Clippy (pedantic) clean.
- Errors that reach the user are `Error` variants with complete, friendly messages.
- Never print or log credential values. `Credentials`' `Debug` output hides the secret.
- No network access in regular tests; use the mock server and fixtures.
- Keep the README written for people who use `covers`, not for developers.
