//! `covers` lists an artist's albums with links to the albums and their
//! cover art, from a Spotify or TIDAL artist link.
//!
//! The binary is a thin shell around [`run`]:
//!
//! ```text
//! cli ──▶ credentials ──▶ providers::{spotify, tidal} ──▶ albums::select ──▶ output
//!                          (over http, with retries)       (filter, dedupe,    (text or
//!                                                           classify, sort)     JSON)
//! ```
//!
//! Network access is confined to [`providers`] and [`http`], and every endpoint
//! is configurable ([`Config`]), so tests run against a local mock server.

pub mod albums;
mod app;
pub mod catalog;
pub mod cli;
pub mod credentials;
pub mod error;
pub mod http;
pub mod link;
pub mod output;
pub mod progress;
pub mod providers;

pub use app::{Config, run};
pub use error::{Error, Result};
