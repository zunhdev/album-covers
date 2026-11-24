---
name: update-rust-toolchain
description: Update the Rust version this project builds with (the channel in rust-toolchain.toml) and, when needed, its minimum supported Rust version (rust-version in Cargo.toml) or edition, keeping Clippy, rustfmt, tests and CI green. Use this whenever the user wants a newer Rust, rustc, cargo, Clippy or toolchain, mentions a new Rust release, asks about MSRV, rust-version or the edition, sees new Clippy warnings after a Rust update, or when a crate upgrade needs a newer Rust than rust-version allows.
---

# Update the Rust toolchain

This project has three separate Rust version settings. They change for different reasons,
so first work out which one the user's request is about:

| Setting | Where | Meaning | When to change |
| ------- | ----- | ------- | -------------- |
| Toolchain | `rust-toolchain.toml` (`channel = "1.100"`) | The Rust used to develop and in CI. rustup installs it on first use in this folder. | Each new stable release (every six weeks), or whenever the user wants. |
| Minimum supported Rust (MSRV) | `Cargo.toml` (`rust-version = "1.87"`) | The oldest Rust that can build `covers`. People running `cargo install` with an older Rust get a clear error. | Only when something needs it: a dependency, or a language or library feature the code wants. |
| Edition | `Cargo.toml` (`edition = "2024"`) | The language edition. | Rarely: a new edition comes about every three years. |

Updating the toolchain doesn't require raising the MSRV, and raising the MSRV doesn't
require a new toolchain. Keep the two apart unless the task needs both.

## Update the toolchain

1. **Find the newest stable release.** Run `rustup check`: for an installed `stable`
   toolchain it says whether an update is available, and to which version. If `stable` isn't
   installed, check the release announcements at https://blog.rust-lang.org/.
2. **Start clean.** Run `git status --short` and `cargo test --locked`, so pre-existing
   failures aren't blamed on the new Rust.
3. **Change the channel** in `rust-toolchain.toml` to the new `major.minor`, e.g.
   `channel = "1.101"`. Use major.minor without a patch number: rustup then takes the latest
   patch release. Keep `components = ["rustfmt", "clippy"]`.
4. **Install it**: `rustup toolchain install`. With no arguments, it reads
   `rust-toolchain.toml`; this is the same command CI uses. Check with `cargo --version`.
5. **Verify:**
   ```sh
   cargo fmt --all --check
   cargo clippy --all-targets --locked -- -D warnings
   cargo test --locked
   RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --locked
   cargo build --release --locked && target/release/covers --version
   ```
   - **New Clippy lints** are the usual work. Fix the code. Silence a lint in
     `[lints.clippy]` in `Cargo.toml` only when it fights a deliberate pattern in this
     codebase, and then add a comment explaining why, like the existing
     `missing_errors_doc` entry.
   - **rustfmt changes:** if `cargo fmt --check` fails, run `cargo fmt` and review the diff.
     It should be formatting only.
6. **Read the release notes** (linked from the announcement). Mention newly stabilized
   features that would simplify code here, but don't refactor unless asked.
7. **Old toolchains:** the previous pinned toolchain stays installed. Offer to remove it with
   `rustup toolchain uninstall 1.100` rather than doing it unasked, since other projects may
   still use it.

CI needs no change: every job runs `rustup toolchain install`, which follows
`rust-toolchain.toml`.

## Raise the minimum supported Rust (MSRV)

Do this only for a concrete reason. Typical ones are:
- `cargo update --dry-run --verbose` shows crates held back with `available: …, requires Rust 1.NN`;
- a crate's major upgrade needs a newer Rust;
- the code wants a newer language or library feature.

1. **Work out the version actually needed.** The highest `rust-version` in the lockfile:
   ```sh
   cargo metadata --format-version 1 --locked | jq -r '[.packages[] | select(.source != null and .rust_version != null) | {name, v: .rust_version}] | max_by(.v | split(".") | map(tonumber)) | "\(.v) (\(.name))"'
   ```
   This counts dev- and platform-specific dependencies too, so it's an upper bound. Then
   add what the code itself uses: the edition's minimum (2024 needs 1.85), let chains in
   `if`/`while` (1.88), and any recently stabilized API. Raise `rust-version` to the highest
   of these, and no further: a lower MSRV lets more people build `covers`.
2. **Edit** `rust-version = "1.NN"` in `Cargo.toml`, keeping exactly that line format: the
   CI job "Minimum supported Rust version" reads it with `sed`.
3. **Let the resolver use it:** run `cargo update` (or go back to the dependency update that
   needed it).
4. **Verify** with the toolchain checks above. To build with the MSRV itself, run
   `cargo +1.NN check --locked`. That needs the toolchain installed, a download of a few
   hundred MB, so ask before running `rustup toolchain install 1.NN --profile minimal`.
   Otherwise say that the CI job will check it.

Never lower `rust-version` without building with that exact version: the number is a promise
to users.

## Change the edition (rare)

Run `cargo fix --edition --all-targets`, then change `edition` in `Cargo.toml`, then run
`cargo fmt` (the style edition follows the edition) and the full verification. Raise
`rust-version` to the new edition's minimum if it's lower. Read the edition guide's
migration notes first: https://doc.rust-lang.org/edition-guide/

## Report

Report briefly:
- which settings changed (from → to);
- lint and formatting fixes;
- anything noteworthy from the release notes;
- the verification results, including anything left to CI (e.g. the MSRV build).

Don't commit unless the user asks.
