//! Database connection configuration loader.
//!
//! Mirrors gaussdb-mcp's config schema exactly
//! (see `rust-opengauss/tools/gaussdb-mcp/src/config.rs`)
//! so that `~/.gaussdb-mcp.toml` works for both tools.
//!
//! # Resolution priority
//!
//! 1. Explicit `--dsn` argument
//! 2. `GAUSSDB_URL` environment variable
//! 3. `DATABASE_URL` environment variable
//! 4. Config file (`--config <path>` or `~/.gaussdb-mcp.toml`)
//!    - Parsed TOML into [`MultiConfig`]
//!    - Picks connection by `--name`, `default_connection`, or first entry
//!    - Handles `password = "keyring"` sentinel (reads from OS keychain)
//! 5. Error with actionable message

use anyhow::{Context, Result};
use keyring::Entry;
use serde::Deserialize;
use std::path::{Path, PathBuf};

pub(crate) const KEYRING_SERVICE: &str = "gaussdb-mcp";
pub(crate) const KEYRING_SENTINEL: &str = "keyring";

// ─── Password source detection ────────────────────────────────────────

/// Describes where a connection's password originated (or will originate).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PasswordSource {
    /// Config file has a plaintext password.
    Plaintext,
    /// Config file has `password = "keyring"` — read from OS keychain.
    Keyring,
    /// Password came from an environment variable.
    EnvVar,
    /// No password at all.
    None,
}

// ─── Connection types (mirror gaussdb-mcp EXACTLY) ────────────────────

/// A single named database connection.
///
/// Fields match `gaussdb-mcp::config::NamedConnection` exactly.
#[derive(Debug, Deserialize, Clone)]
pub struct NamedConnection {
    pub name: String,
    pub url: Option<String>,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub user: Option<String>,
    pub password: Option<String>,
    pub dbname: Option<String>,
    pub sslmode: Option<String>,
    pub statement_timeout: Option<String>,
    pub connection_max_lifetime: Option<String>,
    #[serde(default)]
    pub timeout_action: Option<String>,
}

/// Top-level config file schema.
///
/// Fields match `gaussdb-mcp::config::MultiConfig` exactly.
#[derive(Debug, Deserialize)]
pub struct MultiConfig {
    pub url: Option<String>,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub user: Option<String>,
    pub password: Option<String>,
    pub dbname: Option<String>,
    pub sslmode: Option<String>,
    pub statement_timeout: Option<String>,
    pub connection_max_lifetime: Option<String>,
    pub timeout_action: Option<String>,
    pub default_connection: Option<String>,
    pub connections: Option<Vec<NamedConnection>>,
}

// ─── NamedConnection impl ─────────────────────────────────────────────

impl NamedConnection {
    /// Build the keychain username identifier in `user@host[/dbname]` format.
    ///
    /// Matches `gaussdb-mcp::config::NamedConnection::keyring_username()` exactly.
    pub fn keyring_username(&self) -> String {
        match (&self.user, &self.host, &self.dbname) {
            (Some(u), Some(h), Some(d)) => format!("{}@{}/{}", u, h, d),
            (Some(u), Some(h), None) => format!("{}@{}", u, h),
            (Some(u), _, _) => u.clone(),
            _ => "default".to_string(),
        }
    }

    /// Build a libpq-style space-separated connection URL.
    ///
    /// Format: `host=... port=... user=... password=... dbname=... sslmode=...`
    /// Returns the `url` field directly if set; returns `None` if neither
    /// `host`, `user`, nor `url` is configured.
    ///
    /// Matches `gaussdb-mcp::config::NamedConnection::to_connection_url()` exactly.
    pub fn to_connection_url(&self) -> Option<String> {
        if let Some(ref url) = self.url {
            return Some(url.clone());
        }

        if self.host.is_none() && self.user.is_none() {
            return None;
        }

        let mut parts = Vec::new();
        if let Some(ref host) = self.host {
            parts.push(format!("host={}", host));
        }
        if let Some(port) = self.port {
            parts.push(format!("port={}", port));
        }
        if let Some(ref user) = self.user {
            parts.push(format!("user={}", user));
        }
        if let Some(ref password) = self.password {
            parts.push(format!("password={}", password));
        }
        if let Some(ref dbname) = self.dbname {
            parts.push(format!("dbname={}", dbname));
        }
        if let Some(ref sslmode) = self.sslmode {
            parts.push(format!("sslmode={}", sslmode));
        }

        Some(parts.join(" "))
    }

    /// Determine the password source without accessing the keychain.
    pub fn password_source(&self) -> PasswordSource {
        match self.password.as_deref() {
            Some(p) if p == KEYRING_SENTINEL => PasswordSource::Keyring,
            Some(_) => PasswordSource::Plaintext,
            None => PasswordSource::None,
        }
    }
}

// ─── MultiConfig impl ─────────────────────────────────────────────────

impl MultiConfig {
    /// Resolve the config into a list of connections and an optional default name.
    ///
    /// If `[[connections]]` entries exist, returns them and the
    /// `default_connection` name (falling back to the first entry).
    ///
    /// Otherwise, builds a single "default" connection from flat top-level
    /// fields. Returns an error if no connection data exists at all.
    ///
    /// Matches `gaussdb-mcp::config::MultiConfig::resolve()` exactly.
    pub fn resolve(self) -> Result<(Vec<NamedConnection>, Option<String>)> {
        match self.connections {
            Some(ref conns) if !conns.is_empty() => {
                let default = self
                    .default_connection
                    .clone()
                    .or_else(|| conns.first().map(|c| c.name.clone()));
                Ok((self.connections.unwrap(), default))
            }
            _ => {
                if self.host.is_none() && self.user.is_none() && self.url.is_none() {
                    anyhow::bail!(
                        "config must contain either [[connections]] or flat host/user fields"
                    );
                }
                let single = NamedConnection {
                    name: "default".to_string(),
                    url: self.url,
                    host: self.host,
                    port: self.port,
                    user: self.user,
                    password: self.password,
                    dbname: self.dbname,
                    sslmode: self.sslmode,
                    statement_timeout: self.statement_timeout,
                    connection_max_lifetime: self.connection_max_lifetime,
                    timeout_action: self.timeout_action,
                };
                Ok((vec![single], Some("default".to_string())))
            }
        }
    }
}

// ─── Keyring helpers ──────────────────────────────────────────────────

/// Read a password from the OS keychain for the given username.
///
/// The `username` should be in the format `user@host[/dbname]` as produced by
/// [`NamedConnection::keyring_username`].
pub fn read_keyring_password(username: &str) -> Result<String> {
    let entry = Entry::new(KEYRING_SERVICE, username).context("keyring entry creation failed")?;
    entry.get_password().map_err(|e| {
        anyhow::anyhow!(
            "keyring password not found for '{}'. Store it first:\n  \
             gaussdb-mcp store-password <password> --config <path>\n  \
             or set password in config file as plaintext.\n  \
             Keyring error: {}",
            username,
            e
        )
    })
}

// ─── Config file helpers ──────────────────────────────────────────────

/// Return the default config file path: `~/.gaussdb-mcp.toml`.
fn default_config_path() -> Option<PathBuf> {
    dirs::home_dir().map(|p| p.join(".gaussdb-mcp.toml"))
}

/// Find the config file path, returning an error if none exists.
///
/// If `config_path` is `Some`, uses it directly.
/// Otherwise checks `~/.gaussdb-mcp.toml`.
fn find_config_path(config_path: Option<&Path>) -> Result<PathBuf> {
    match config_path {
        Some(p) => Ok(p.to_path_buf()),
        None => match default_config_path() {
            Some(p) if p.exists() => Ok(p),
            _ => anyhow::bail!(
                "No connection configuration found. Use --dsn, --config, \
                 set GAUSSDB_URL/DATABASE_URL env var, or create ~/.gaussdb-mcp.toml"
            ),
        },
    }
}

// ─── Public API ───────────────────────────────────────────────────────

/// Resolve a database connection DSN from explicit args, env vars, or config file.
///
/// Priority (first wins):
/// 1. `dsn` argument (if `Some` and non-empty after trimming)
/// 2. `GAUSSDB_URL` environment variable
/// 3. `DATABASE_URL` environment variable
/// 4. Config file:
///    - Uses `config_path` or defaults to `~/.gaussdb-mcp.toml`
///    - Parses [`MultiConfig`] from TOML
///    - Picks connection by `name` arg, `default_connection` field, or first entry
///    - Resolves `"keyring"` password sentinel via OS keychain
/// 5. Returns error with actionable message
///
/// # Errors
/// Returns an error if no DSN source is available, config file is missing or
/// malformed, the named connection is not found, or keychain lookup fails.
pub fn resolve_dsn(
    dsn: Option<&str>,
    config_path: Option<&Path>,
    name: Option<&str>,
) -> Result<String> {
    // 1. Explicit DSN override
    if let Some(d) = dsn {
        let trimmed = d.trim();
        if !trimmed.is_empty() {
            return Ok(trimmed.to_string());
        }
    }

    // 2–3. Environment variables (GAUSSDB_URL > DATABASE_URL)
    if let Ok(url) = std::env::var("GAUSSDB_URL").or_else(|_| std::env::var("DATABASE_URL")) {
        if !url.trim().is_empty() {
            return Ok(url);
        }
    }

    // 4. Config file
    let config_path = find_config_path(config_path)?;
    let content = std::fs::read_to_string(&config_path).context(format!(
        "Failed to read config file: {}",
        config_path.display()
    ))?;
    let config: MultiConfig = toml::from_str(&content).context(format!(
        "Failed to parse config file: {}",
        config_path.display()
    ))?;

    let (connections, default_name) = config.resolve()?;

    // Pick the target connection
    let target_name = match name {
        Some(n) => n.to_string(),
        None => default_name.unwrap_or_else(|| {
            connections
                .first()
                .map(|c| c.name.clone())
                .unwrap_or_default()
        }),
    };

    let conn = connections
        .iter()
        .find(|c| c.name == target_name)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Connection '{}' not found in config file. Available: {:?}",
                target_name,
                connections.iter().map(|c| &c.name).collect::<Vec<_>>()
            )
        })?;

    // Handle keyring sentinel
    let mut resolved_conn = conn.clone();
    if resolved_conn.password_source() == PasswordSource::Keyring {
        let ring_user = resolved_conn.keyring_username();
        let pw = read_keyring_password(&ring_user)?;
        resolved_conn.password = Some(pw);
    }

    // Build the connection URL
    resolved_conn.to_connection_url().ok_or_else(|| {
        anyhow::anyhow!(
            "Connection '{}' must contain either `url` or at least `host`/`user` fields",
            resolved_conn.name
        )
    })
}

// ─── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    // ── Password source detection ───────────────────────────────────

    #[test]
    fn test_plaintext_password_detected() {
        let conn = NamedConnection {
            name: "test".to_string(),
            url: None,
            host: Some("localhost".to_string()),
            port: None,
            user: Some("admin".to_string()),
            password: Some("supersecret".to_string()),
            dbname: None,
            sslmode: None,
            statement_timeout: None,
            connection_max_lifetime: None,
            timeout_action: None,
        };
        assert_eq!(conn.password_source(), PasswordSource::Plaintext);
    }

    #[test]
    fn test_keyring_sentinel_detected() {
        let conn = NamedConnection {
            name: "test".to_string(),
            url: None,
            host: Some("localhost".to_string()),
            port: None,
            user: Some("admin".to_string()),
            password: Some("keyring".to_string()),
            dbname: None,
            sslmode: None,
            statement_timeout: None,
            connection_max_lifetime: None,
            timeout_action: None,
        };
        assert_eq!(conn.password_source(), PasswordSource::Keyring);
    }

    #[test]
    fn test_no_password_detected() {
        let conn = NamedConnection {
            name: "test".to_string(),
            url: None,
            host: Some("localhost".to_string()),
            port: None,
            user: Some("admin".to_string()),
            password: None,
            dbname: None,
            sslmode: None,
            statement_timeout: None,
            connection_max_lifetime: None,
            timeout_action: None,
        };
        assert_eq!(conn.password_source(), PasswordSource::None);
    }

    // ── keyring_username ────────────────────────────────────────────

    #[test]
    fn test_keyring_username_full() {
        let conn = NamedConnection {
            name: "test".to_string(),
            url: None,
            host: Some("db.example.com".to_string()),
            port: None,
            user: Some("admin".to_string()),
            password: None,
            dbname: Some("mydb".to_string()),
            sslmode: None,
            statement_timeout: None,
            connection_max_lifetime: None,
            timeout_action: None,
        };
        assert_eq!(conn.keyring_username(), "admin@db.example.com/mydb");
    }

    #[test]
    fn test_keyring_username_no_db() {
        let conn = NamedConnection {
            name: "test".to_string(),
            url: None,
            host: Some("localhost".to_string()),
            port: None,
            user: Some("postgres".to_string()),
            password: None,
            dbname: None,
            sslmode: None,
            statement_timeout: None,
            connection_max_lifetime: None,
            timeout_action: None,
        };
        assert_eq!(conn.keyring_username(), "postgres@localhost");
    }

    #[test]
    fn test_keyring_username_no_host() {
        let conn = NamedConnection {
            name: "test".to_string(),
            url: None,
            host: None,
            port: None,
            user: Some("postgres".to_string()),
            password: None,
            dbname: None,
            sslmode: None,
            statement_timeout: None,
            connection_max_lifetime: None,
            timeout_action: None,
        };
        assert_eq!(conn.keyring_username(), "postgres");
    }

    #[test]
    fn test_keyring_username_default() {
        let conn = NamedConnection {
            name: "test".to_string(),
            url: None,
            host: None,
            port: None,
            user: None,
            password: None,
            dbname: None,
            sslmode: None,
            statement_timeout: None,
            connection_max_lifetime: None,
            timeout_action: None,
        };
        assert_eq!(conn.keyring_username(), "default");
    }

    // ── to_connection_url ───────────────────────────────────────────

    #[test]
    fn test_url_field_used_directly() {
        let conn = NamedConnection {
            name: "test".to_string(),
            url: Some("postgresql://localhost/mydb".to_string()),
            host: Some("ignored".to_string()),
            port: None,
            user: Some("ignored".to_string()),
            password: None,
            dbname: None,
            sslmode: None,
            statement_timeout: None,
            connection_max_lifetime: None,
            timeout_action: None,
        };
        assert_eq!(
            conn.to_connection_url(),
            Some("postgresql://localhost/mydb".to_string())
        );
    }

    #[test]
    fn test_to_connection_url_basic() {
        let conn = NamedConnection {
            name: "test".to_string(),
            url: None,
            host: Some("localhost".to_string()),
            port: Some(5432),
            user: Some("postgres".to_string()),
            password: Some("secret".to_string()),
            dbname: Some("mydb".to_string()),
            sslmode: Some("disable".to_string()),
            statement_timeout: None,
            connection_max_lifetime: None,
            timeout_action: None,
        };
        let url = conn.to_connection_url().unwrap();
        assert!(url.contains("host=localhost"));
        assert!(url.contains("port=5432"));
        assert!(url.contains("user=postgres"));
        assert!(url.contains("password=secret"));
        assert!(url.contains("dbname=mydb"));
        assert!(url.contains("sslmode=disable"));
    }

    #[test]
    fn test_to_connection_url_no_host_or_user_returns_none() {
        let conn = NamedConnection {
            name: "test".to_string(),
            url: None,
            host: None,
            port: None,
            user: None,
            password: None,
            dbname: None,
            sslmode: None,
            statement_timeout: None,
            connection_max_lifetime: None,
            timeout_action: None,
        };
        assert_eq!(conn.to_connection_url(), None);
    }

    // ── MultiConfig resolve ─────────────────────────────────────────

    #[test]
    fn test_flat_config_loads() {
        let toml_str = r#"
host = "localhost"
user = "postgres"
dbname = "mydb"
"#;
        let config: MultiConfig = toml::from_str(toml_str).unwrap();
        let (conns, default) = config.resolve().unwrap();
        assert_eq!(conns.len(), 1);
        assert_eq!(conns[0].name, "default");
        assert_eq!(conns[0].host.as_deref(), Some("localhost"));
        assert_eq!(default.as_deref(), Some("default"));
        let url = conns[0].to_connection_url().unwrap();
        assert!(url.contains("host=localhost"));
        assert!(url.contains("user=postgres"));
        assert!(url.contains("dbname=mydb"));
    }

    #[test]
    fn test_multi_connections_picks_default() {
        let toml_str = r#"
default_connection = "prod"

[[connections]]
name = "dev"
host = "dev.example.com"
user = "dev_user"
dbname = "dev_db"

[[connections]]
name = "prod"
host = "prod.example.com"
user = "prod_user"
dbname = "prod_db"
"#;
        let config: MultiConfig = toml::from_str(toml_str).unwrap();
        let (conns, default) = config.resolve().unwrap();
        assert_eq!(conns.len(), 2);
        assert_eq!(default.as_deref(), Some("prod"));
    }

    #[test]
    fn test_multi_connections_picks_by_name() {
        let toml_str = r#"
default_connection = "prod"

[[connections]]
name = "dev"
host = "dev.example.com"
user = "dev_user"
dbname = "dev_db"

[[connections]]
name = "stage"
host = "stage.example.com"
user = "stage_user"
dbname = "stage_db"
"#;
        let config: MultiConfig = toml::from_str(toml_str).unwrap();
        let (conns, _) = config.resolve().unwrap();
        let dev = conns.iter().find(|c| c.name == "dev").unwrap();
        let url = dev.to_connection_url().unwrap();
        assert!(url.contains("host=dev.example.com"));
        assert!(url.contains("user=dev_user"));
    }

    #[test]
    fn test_multi_connections_fallback_to_first() {
        let toml_str = r#"
[[connections]]
name = "alpha"
host = "alpha.example.com"
user = "alpha_user"
dbname = "alpha_db"

[[connections]]
name = "beta"
host = "beta.example.com"
user = "beta_user"
dbname = "beta_db"
"#;
        let config: MultiConfig = toml::from_str(toml_str).unwrap();
        let (conns, default) = config.resolve().unwrap();
        assert_eq!(default.as_deref(), Some("alpha"));
        assert_eq!(conns.first().unwrap().name, "alpha");
    }

    #[test]
    fn test_empty_flat_config_errors() {
        let toml_str = "# empty\n";
        let config: MultiConfig = toml::from_str(toml_str).unwrap();
        let result = config.resolve();
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("config must contain either")
                || err.contains("connections")
                || err.contains("host/user")
        );
    }

    // ── resolve_dsn integration tests ───────────────────────────────

    #[test]
    fn test_dsn_overrides_everything() {
        let result = resolve_dsn(
            Some("host=override user=override"),
            Some(Path::new("/nonexistent/config.toml")),
            None,
        );
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "host=override user=override");
    }

    #[test]
    fn test_missing_config_file_errors() {
        let result = resolve_dsn(None, Some(Path::new("/nonexistent/config.toml")), None);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("/nonexistent/config.toml")
                || err.contains("No connection configuration found")
        );
    }

    #[test]
    fn test_no_config_source_errors() {
        // Save and clear env vars to avoid GAUSSDB_URL/DATABASE_URL interference
        let gaussdb_orig = std::env::var("GAUSSDB_URL").ok();
        let database_orig = std::env::var("DATABASE_URL").ok();
        std::env::remove_var("GAUSSDB_URL");
        std::env::remove_var("DATABASE_URL");

        // Use a temp file with empty/insufficient config so we don't
        // accidentally hit ~/.gaussdb-mcp.toml or the OS keychain
        let mut tmpfile = tempfile::NamedTempFile::new().unwrap();
        writeln!(tmpfile, "# empty — no connections, no host/user").unwrap();
        let path = tmpfile.path().to_path_buf();

        let result = resolve_dsn(None, Some(&path), None);

        // Restore env vars
        if let Some(v) = gaussdb_orig {
            std::env::set_var("GAUSSDB_URL", v);
        }
        if let Some(v) = database_orig {
            std::env::set_var("DATABASE_URL", v);
        }

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(!err.is_empty());
        assert!(
            err.contains("connections")
                || err.contains("host/user")
                || err.contains("config")
                || err.contains("not found")
        );
    }

    #[test]
    fn test_connection_not_found_by_name_errors() {
        let mut tmpfile = tempfile::NamedTempFile::new().unwrap();
        write!(
            tmpfile,
            r#"
[[connections]]
name = "alpha"
host = "alpha.host"
user = "alpha_user"
dbname = "alpha_db"
"#
        )
        .unwrap();
        let path = tmpfile.path().to_path_buf();

        let result = resolve_dsn(None, Some(&path), Some("nonexistent"));
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("not found") || err.contains("nonexistent"));
    }

    #[test]
    fn test_config_picks_by_name() {
        let mut tmpfile = tempfile::NamedTempFile::new().unwrap();
        write!(
            tmpfile,
            r#"
[[connections]]
name = "dev"
host = "dev.example.com"
user = "dev_user"
dbname = "dev_db"
port = 5432

[[connections]]
name = "prod"
host = "prod.example.com"
user = "prod_user"
password = "prod_pass"
dbname = "prod_db"
port = 5432
sslmode = "disable"
"#
        )
        .unwrap();
        let path = tmpfile.path().to_path_buf();

        let result = resolve_dsn(None, Some(&path), Some("prod"));
        assert!(result.is_ok());
        let url = result.unwrap();
        assert!(url.contains("host=prod.example.com"));
        assert!(url.contains("user=prod_user"));
        assert!(url.contains("password=prod_pass"));
        assert!(url.contains("dbname=prod_db"));
        assert!(url.contains("sslmode=disable"));
    }

    #[test]
    fn test_config_picks_first_when_no_name() {
        let mut tmpfile = tempfile::NamedTempFile::new().unwrap();
        write!(
            tmpfile,
            r#"
[[connections]]
name = "default"
host = "default.host"
user = "default_user"
dbname = "default_db"

[[connections]]
name = "other"
host = "other.host"
user = "other_user"
dbname = "other_db"
"#
        )
        .unwrap();
        let path = tmpfile.path().to_path_buf();

        let result = resolve_dsn(None, Some(&path), None);
        assert!(result.is_ok());
        let url = result.unwrap();
        assert!(url.contains("host=default.host"));
    }
}
