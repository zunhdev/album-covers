---
description: Project conventions for the album-covers Rust CLI.
globs: "*.rs, Cargo.toml, README.md, CONTRIBUTING.md"
alwaysApply: true
---

`covers` is a small Rust CLI that prints album and cover links for a Spotify or TIDAL artist.
CONTRIBUTING.md describes the architecture; the provider modules document each API's quirks.

- The toolchain is pinned in `rust-toolchain.toml`; `rust-version` in `Cargo.toml` is the
  checked minimum. Read dependency sources in `~/.cargo/registry` when unsure about an API.
- `main.rs` stays thin. `run` receives everything that differs between production and tests
  through `Config` (credentials, endpoints, retry policy) and its writers.
- Never call real APIs from regular tests. Use mockito with recorded responses in
  `tests/fixtures/`. Live checks belong in `tests/live.rs` (`#[ignore]`).
- Errors the user may see are `Error` variants with complete, friendly messages.
- Credentials come from the environment, `./.env`, or the per-user `credentials.env`. Never
  print secrets, never commit real ones, and never read other projects' `.env` files.
- The README is for end users; developer documentation goes in CONTRIBUTING.md.
- Before committing: `cargo fmt`, `cargo clippy --all-targets` (no warnings), `cargo test`,
  `cargo deny check`.
- Updating crates or Rust: follow `.claude/skills/update-dependencies/SKILL.md` and
  `.claude/skills/update-rust-toolchain/SKILL.md`.

## Common commands

```sh
cargo run -- "<artist link>"
cargo test
cargo clippy --all-targets
cargo fmt
cargo deny check
cargo test --test live -- --ignored
```
