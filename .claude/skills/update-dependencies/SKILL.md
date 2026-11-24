---
name: update-dependencies
description: Update this project's Rust crate dependencies, including major-version upgrades. Covers surveying what is outdated, routine in-range updates with cargo update, one-at-a-time major upgrades with migration guided by changelogs, and the full verification pass (fmt, clippy, tests, docs, cargo-deny, release build, live check). Use this whenever the user wants to update, bump or upgrade crates, dependencies, packages or Cargo.lock, asks what is outdated, mentions a RUSTSEC advisory or a cargo-deny failure, or has a Dependabot pull request that fails CI, even if they don't say "dependencies".
---

# Update dependencies

The goal is the newest versions of every crate, majors included, with the
behavior of `covers` unchanged and every check green. Updates go in small,
verified steps, so that when something breaks, the cause is obvious.

## Background

- `Cargo.toml` holds version *ranges*: `clap = "4.6.9"` means `>=4.6.9, <5`. `Cargo.lock`
  pins the exact versions that are built. `cargo update` moves the lockfile within the
  ranges; a major upgrade changes the range in `Cargo.toml`.
- For `0.x` crates the minor version is the breaking one: `0.23 → 0.24` is a major upgrade.
- The resolver respects `rust-version` in `Cargo.toml` (the minimum supported Rust). It
  won't pick a release that needs a newer Rust, and says so with an `available:` note. If an
  upgrade needs a newer Rust, that's the `update-rust-toolchain` skill's job.
- Once the project is on GitHub, Dependabot opens a weekly pull request with in-range updates.
  Manual runs of this workflow are mostly about major upgrades and failing Dependabot PRs.

## Workflow

### 1. Start from a clean state

Run `git status --short`. If there are unrelated uncommitted changes, tell the user before
going on: mixing them with dependency updates makes both harder to review. Don't stash or
discard anything without asking. Then run `cargo test --locked` to confirm everything passes
*before* updating; otherwise a pre-existing failure looks like an update problem.

### 2. Survey

```sh
.claude/skills/update-dependencies/scripts/outdated.sh   # direct deps: locked vs newest, MAJOR or compatible
cargo update --dry-run --verbose                          # what an in-range update would change
```

Look for `available:` notes in the dry run. They mark crates held back by `rust-version`.
Show the user the plan: in-range updates, the list of major upgrades, anything held back.

### 3. In-range updates

```sh
cargo update
```

Then verify (step 5). Doing this before the majors gives them a clean, current baseline.

### 4. Major upgrades, one crate at a time

Start with test-only crates (`[dev-dependencies]`): they can't affect users, so they are the
cheapest. For each crate:

1. **Read what changed.** Find the changelog: after `cargo fetch`, look in
   `~/.cargo/registry/src/*/<crate>-<new version>/` (`CHANGELOG.md`, `RELEASES.md`), or follow
   the repository link from `cargo info <crate>@<new version>`. Read every breaking change
   between the locked version and the new one, and compare it with
   [references/crate-notes.md](references/crate-notes.md), which lists what this project
   relies on in each crate, including behaviors no test covers.
2. **Upgrade:** `cargo add <crate>@<new major>` (add `--dev` for dev-dependencies). Check
   the `Cargo.toml` diff: existing features (e.g. clap's `derive`, `wrap_help`) must stay.
3. **Migrate.** Fix compile errors the way the changelog or migration guide says, rather than
   by trial and error; the crate's own source in the registry is the ground truth for the new
   API. Keep behavior identical. The tests define it, so don't loosen a test to make it pass.
   If an expectation really must change, for example clap's help formatting, make sure the
   change is cosmetic and intended, and mention it in the report.
4. **Verify** (step 5), then continue with the next crate.

Stop and report instead of forcing it when an upgrade:
- needs a newer Rust than `rust-version`;
- is blocked by another crate that doesn't support the new version yet (e.g. mockito pinned
  to an older hyper);
- would need a redesign rather than a migration.

A held-back major is a fine outcome when the reason is written down.

### 5. Verify

Run after the in-range update and after each major upgrade:

```sh
cargo fmt --all --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --locked
cargo deny check
cargo build --release --locked && target/release/covers --version
```

- **cargo-deny.** If `cargo deny` isn't available, use `mise x cargo-deny@latest -- cargo-deny
  check` when mise is installed, or ask before running `cargo install --locked cargo-deny`.
  - A new license: allow it in `deny.toml` only if it's permissive. A test-only crate can get
    a crate-specific exception instead, like `colored`.
  - An advisory: upgrade past it, or report it.
  - Warnings about duplicate crate versions are acceptable.
- **Live check.** After upgrades that touch HTTP, TLS, JSON decoding or credentials (ureq,
  rustls, serde, dotenvy, etcetera), also run the live TIDAL test:
  `cargo test --test live -- --ignored tidal`. It needs TIDAL credentials and network access,
  and a firewall such as Little Snitch may hold the new test binary until the user allows
  it, so use a timeout and tell the user if it hangs. Skip the Spotify live test unless the
  user says their Spotify app works: it needs a Premium subscription.
- **Checks by hand.** [references/crate-notes.md](references/crate-notes.md) lists behaviors
  that need one after certain upgrades, such as the credentials path after an etcetera upgrade.

### 6. Report

Finish with a short report:
- a table of crate, from → to, and in-range or major;
- code changes made for each major upgrade, and any user-visible differences (help text,
  messages, output);
- crates held back, and why;
- the verification results, including what wasn't run (e.g. the Spotify live test).

Don't commit unless the user asks. If they do, a message such as
`Update dependencies (clap 5, ureq 4)` works, with the attribution lines this session uses.
