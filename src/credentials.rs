//! Finding a service's client ID and secret.
//!
//! Each variable (`TIDAL_CLIENT_ID`, …) is taken from the first of these
//! places that has a non-empty value:
//!
//! 1. the environment,
//! 2. `.env` in the current directory,
//! 3. `credentials.env` in the user's config directory
//!    (`~/.config/album-covers/` on macOS and Linux, honoring
//!    `XDG_CONFIG_HOME`; `%APPDATA%\album-covers\` on Windows).

use std::collections::HashMap;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use etcetera::BaseStrategy;

use crate::catalog::Service;
use crate::error::{Error, Result};

/// Directory name inside the user's config directory.
pub const CONFIG_DIR_NAME: &str = "album-covers";
/// File name of the per-user credentials file.
pub const CONFIG_FILE_NAME: &str = "credentials.env";

/// The variables this program reads.
const VARIABLES: [&str; 4] = [
    "SPOTIFY_CLIENT_ID",
    "SPOTIFY_CLIENT_SECRET",
    "TIDAL_CLIENT_ID",
    "TIDAL_CLIENT_SECRET",
];

/// OAuth client credentials for one service.
#[derive(Clone, PartialEq, Eq)]
pub struct Credentials {
    pub client_id: String,
    pub client_secret: String,
}

impl fmt::Debug for Credentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Credentials")
            .field("client_id", &self.client_id)
            .field("client_secret", &"<redacted>")
            .finish()
    }
}

/// Places to look for credentials, highest priority first.
#[derive(Clone, Debug, Default)]
pub struct Sources {
    layers: Vec<HashMap<String, String>>,
    /// The per-user credentials file, suggested in error messages.
    config_file: Option<PathBuf>,
}

impl Sources {
    /// The standard places: the environment, `./.env`, then the user's credentials file.
    pub fn standard() -> Result<Self> {
        let mut sources = Self::default()
            .with_vars(environment_variables())
            .with_file(Path::new(".env"))?;
        if let Some(path) = user_config_file() {
            sources = sources.with_file(&path)?;
            sources.config_file = Some(path);
        }
        Ok(sources)
    }

    /// Adds variables, with lower priority than everything added before.
    #[must_use]
    pub fn with_vars(mut self, vars: impl IntoIterator<Item = (String, String)>) -> Self {
        self.layers.push(vars.into_iter().collect());
        self
    }

    /// Adds the variables of a `.env`-style file, if it exists.
    pub fn with_file(self, path: &Path) -> Result<Self> {
        let vars = read_env_file(path)?;
        Ok(self.with_vars(vars))
    }

    /// Looks up `service`'s credentials.
    pub fn credentials(&self, service: Service) -> Result<Credentials> {
        let id_name = format!("{}_CLIENT_ID", service.env_prefix());
        let secret_name = format!("{}_CLIENT_SECRET", service.env_prefix());
        match (self.get(&id_name), self.get(&secret_name)) {
            (Some(client_id), Some(client_secret)) => Ok(Credentials {
                client_id: client_id.to_owned(),
                client_secret: client_secret.to_owned(),
            }),
            (client_id, client_secret) => {
                let missing = [(client_id, id_name), (client_secret, secret_name)]
                    .into_iter()
                    .filter_map(|(value, name)| value.is_none().then_some(name))
                    .collect();
                Err(Error::MissingCredentials {
                    service,
                    missing,
                    config_file: self.config_file.clone(),
                })
            }
        }
    }

    fn get(&self, name: &str) -> Option<&str> {
        self.layers.iter().find_map(|vars| {
            vars.get(name)
                .map(String::as_str)
                .filter(|value| !value.is_empty())
        })
    }
}

/// `credentials.env` in the user's config directory, if there is a home directory.
#[must_use]
pub fn user_config_file() -> Option<PathBuf> {
    let base = etcetera::choose_base_strategy().ok()?;
    Some(
        base.config_dir()
            .join(CONFIG_DIR_NAME)
            .join(CONFIG_FILE_NAME),
    )
}

/// The credential variables that are set in the process environment.
fn environment_variables() -> impl Iterator<Item = (String, String)> {
    VARIABLES
        .into_iter()
        .filter_map(|name| Some((name.to_owned(), std::env::var(name).ok()?)))
}

/// Reads a `.env`-style file. A missing file has no variables.
fn read_env_file(path: &Path) -> Result<HashMap<String, String>> {
    let error = |reason: String| Error::CredentialsFile {
        path: path.to_owned(),
        reason,
    };
    let iter = match dotenvy::from_path_iter(path) {
        Ok(iter) => iter,
        Err(dotenvy::Error::Io(err)) if err.kind() == io::ErrorKind::NotFound => {
            return Ok(HashMap::new());
        }
        Err(err) => return Err(error(err.to_string())),
    };
    iter.map(|item| {
        item.map_err(|err| match err {
            // The parser reports the offending line's text, which may hold a
            // secret, so only its number is shown.
            dotenvy::Error::LineParse(line, _) => match line_number(path, &line) {
                Some(number) => error(format!("line {number} is not in KEY=value form")),
                None => error("a line is not in KEY=value form".to_owned()),
            },
            other => error(other.to_string()),
        })
    })
    .collect()
}

fn line_number(path: &Path, line: &str) -> Option<usize> {
    let text = std::fs::read_to_string(path).ok()?;
    let first = line.lines().next()?;
    text.lines()
        .position(|candidate| candidate == first)
        .map(|index| index + 1)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    fn vars(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect()
    }

    #[test]
    fn earlier_sources_win_and_empty_values_count_as_missing() {
        let sources = Sources::default()
            .with_vars(vars(&[
                ("TIDAL_CLIENT_ID", "from-env"),
                ("TIDAL_CLIENT_SECRET", ""),
            ]))
            .with_vars(vars(&[
                ("TIDAL_CLIENT_ID", "from-file"),
                ("TIDAL_CLIENT_SECRET", "secret-from-file"),
            ]));
        let credentials = sources.credentials(Service::Tidal).unwrap();
        assert_eq!(credentials.client_id, "from-env");
        assert_eq!(credentials.client_secret, "secret-from-file");
    }

    #[test]
    fn missing_credentials_explain_what_to_set_and_where() {
        let sources = Sources {
            config_file: Some(PathBuf::from(
                "/home/me/.config/album-covers/credentials.env",
            )),
            ..Sources::default().with_vars(vars(&[("SPOTIFY_CLIENT_ID", "id")]))
        };
        let message = sources
            .credentials(Service::Spotify)
            .unwrap_err()
            .to_string();
        assert_eq!(
            message,
            "missing Spotify credentials: SPOTIFY_CLIENT_SECRET\n\n\
             Create an app at https://developer.spotify.com/dashboard to get them, then put them in\n  \
             /home/me/.config/album-covers/credentials.env\n\
             (or in .env in the current directory, or in environment variables). For example:\n\n  \
             SPOTIFY_CLIENT_ID=your-client-id\n  SPOTIFY_CLIENT_SECRET=your-client-secret"
        );

        let none = Sources::default()
            .credentials(Service::Tidal)
            .unwrap_err()
            .to_string();
        assert!(
            none.starts_with("missing TIDAL credentials: TIDAL_CLIENT_ID, TIDAL_CLIENT_SECRET\n"),
            "{none}"
        );
    }

    #[test]
    fn reads_env_files_and_ignores_missing_ones() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("credentials.env");
        fs::write(&path, "# comment\nexport TIDAL_CLIENT_ID=abc\nTIDAL_CLIENT_SECRET='s3cr=t' # inline comment\n").unwrap();

        let sources = Sources::default()
            .with_file(&path)
            .unwrap()
            .with_file(&dir.path().join("missing.env"))
            .unwrap();
        let credentials = sources.credentials(Service::Tidal).unwrap();
        assert_eq!(
            credentials,
            Credentials {
                client_id: "abc".into(),
                client_secret: "s3cr=t".into()
            }
        );
    }

    #[test]
    fn malformed_files_report_the_line_number_but_not_its_content() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad.env");
        fs::write(&path, "TIDAL_CLIENT_ID=abc\nTIDAL_CLIENT_SECRET hunter2\n").unwrap();

        let message = Sources::default().with_file(&path).unwrap_err().to_string();
        assert_eq!(
            message,
            format!(
                "can't read {}: line 2 is not in KEY=value form",
                path.display()
            )
        );
        assert!(!message.contains("hunter2"));
    }

    #[test]
    fn debug_output_hides_the_secret() {
        let credentials = Credentials {
            client_id: "id".into(),
            client_secret: "hunter2".into(),
        };
        assert!(!format!("{credentials:?}").contains("hunter2"));
    }
}
