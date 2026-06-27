//! Database connection and EXPLAIN fetching.
//!
//! Connects to an OpenGauss/GaussDB instance, runs EXPLAIN (or EXPLAIN ANALYZE),
//! and returns the raw TEXT output suitable for feeding into `ogexplain_core::parse()`.
//!
//! Connection resolution (config file parsing, keychain, TLS/sslmode selection) is
//! handled entirely by [`gaussdb::config::connect_sync`]. This module never sees the
//! DSN/connection string directly; it only forwards the optional config-file path
//! and named-connection selector, then translates the resulting [`ConnectError`]
//! into actionable, user-facing diagnostics via [`map_connect_error`].

use anyhow::{Context, Result};
use std::path::Path;

fn build_explain_sql(sql: &str, analyze: bool) -> String {
    let prefix = if analyze {
        "EXPLAIN ANALYZE"
    } else {
        "EXPLAIN"
    };
    format!("{prefix} {sql}")
}

/// Fetch EXPLAIN TEXT output from a database.
///
/// # Arguments
/// * `config_path` - Path to config file (defaults to `~/.gaussdb-mcp.toml`)
/// * `name` - Named connection selector from config file
/// * `sql` - The SQL statement to explain
/// * `analyze` - If true, run EXPLAIN ANALYZE instead of EXPLAIN
///
/// # Errors
/// Returns error if connection fails or EXPLAIN execution fails.
pub fn fetch_explain(
    config_path: Option<&Path>,
    name: Option<&str>,
    sql: &str,
    analyze: bool,
) -> Result<String> {
    #[cfg(not(feature = "db"))]
    {
        let _ = (config_path, name, sql, analyze);
        anyhow::bail!("Database support not compiled. Rebuild with --features db");
    }

    #[cfg(feature = "db")]
    {
        fetch_explain_impl(config_path, name, sql, analyze)
    }
}

#[cfg(feature = "db")]
fn fetch_explain_impl(
    config_path: Option<&Path>,
    name: Option<&str>,
    sql: &str,
    analyze: bool,
) -> Result<String> {
    let debug_raw = std::env::var_os("OGEXPLAIN_DEBUG_RAW").is_some();

    let mut client = gaussdb::config::connect_sync(None, config_path, name)
        .map_err(|err| map_connect_error(err, config_path, name))?;

    let explain_sql = build_explain_sql(sql, analyze);

    // Use the extended-query protocol (client.query) instead of the simple-query
    // protocol (client.simple_query). The extended protocol is the same path used
    // by gaussdb-mcp cli / psql / gsql for display, and is reliably implemented
    // across OG versions for EXPLAIN's text-column result. The simple-query
    // protocol has been observed to return unexpected row content for EXPLAIN
    // on some OG builds, producing "No plan nodes found" downstream.
    let rows = client
        .query(&explain_sql, &[])
        .context("Failed to execute EXPLAIN")?;

    if debug_raw {
        dump_query_rows(&rows);
    }

    let mut output = String::new();
    for row in &rows {
        let line: Option<String> = row.get(0);
        if let Some(text) = line {
            output.push_str(&text);
            output.push('\n');
        }
    }

    let trimmed = output.trim_end().to_string();

    if debug_raw {
        dump_final_output(&trimmed);
    }

    if trimmed.is_empty() {
        anyhow::bail!("EXPLAIN returned empty result");
    }

    Ok(trimmed)
}

/// Debug-only dump: show each binary-protocol Row with full byte visibility.
///
/// Activated by setting `OGEXPLAIN_DEBUG_RAW=1`. Useful for diagnosing parse
/// failures where the fetched text differs from what a psql/gsql client would
/// display (BOM, ideographic spaces, unexpected column counts/types, etc.).
#[cfg(feature = "db")]
fn dump_query_rows(rows: &[gaussdb::sync::Row]) {
    eprintln!("--- OGEXPLAIN_DEBUG_RAW: {} rows ---", rows.len());
    for (i, row) in rows.iter().enumerate() {
        let cols = row.len();
        let line: Option<String> = row.get(0);
        let col0 = line.as_deref().unwrap_or("<NULL>");
        eprintln!(
            "[{:3}] Row (cols={}, col0_len={}): {:?}",
            i,
            cols,
            col0.len(),
            col0
        );
    }
}

/// Debug-only dump: show the final trimmed string with visible whitespace.
#[cfg(feature = "db")]
fn dump_final_output(trimmed: &str) {
    eprintln!(
        "--- OGEXPLAIN_DEBUG_RAW: final trimmed output = {} bytes, {} lines ---",
        trimmed.len(),
        trimmed.lines().count()
    );

    let bytes = trimmed.as_bytes();
    let show = bytes.len().min(64);
    eprintln!("--- hex of first {} bytes ---", show);
    for chunk in bytes[..show].chunks(16) {
        let hex: String = chunk.iter().map(|b| format!("{:02x} ", b)).collect();
        let ascii: String = chunk
            .iter()
            .map(|&b| if (32..127).contains(&b) { b as char } else { '.' })
            .collect();
        eprintln!("  {:48} {}", hex, ascii);
    }

    eprintln!("--- per-line (visible whitespace) ---");
    for (i, line) in trimmed.lines().enumerate() {
        let visible: String = line
            .chars()
            .map(|c| match c {
                '\t' => "\\t".to_string(),
                '\r' => "\\r".to_string(),
                '\u{00A0}' => "[NBSP]".to_string(),
                '\u{202F}' => "[NNBSP]".to_string(),
                '\u{3000}' => "[IDEOGRAPHIC_SPACE]".to_string(),
                '\u{FEFF}' => "[BOM]".to_string(),
                c if (c as u32) < 0x20 => format!("[^{:02x}]", c as u32),
                c => c.to_string(),
            })
            .collect();
        eprintln!("[{:3}|len={:3}] {}", i + 1, line.len(), visible);
    }
    eprintln!("--- end debug dump ---");
}

/// Translate a [`gaussdb::config::ConnectError`] into a specific, actionable
/// [`anyhow::Error`]. The original error is preserved as the source so the full
/// chain is still available via `anyhow`'s `Caused by:` rendering.
///
/// This exists because the upstream `ConnectError::Driver(Box<dyn Error>)` variant
/// does not annotate `#[source]`, so its underlying cause chain is otherwise lost
/// when the error crosses the crate boundary.
#[cfg(feature = "db")]
fn map_connect_error(
    err: gaussdb::config::ConnectError,
    config_path: Option<&Path>,
    name: Option<&str>,
) -> anyhow::Error {
    use gaussdb::config::{ConfigError, ConnectError};

    let target = name.unwrap_or("default");
    let cfg_display = config_path
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "~/.gaussdb-mcp.toml (default search path)".to_string());

    let context_msg = match &err {
        ConnectError::Config(ConfigError::ConfigNotFound { searched_path }) => format!(
            "Config file not found at {} (target: connection '{}')\n  \
             Fix: create ~/.gaussdb-mcp.toml with host/user/password/dbname fields,\n  \
             or pass --config <path> to point at an existing config file,\n  \
             or set the GAUSSDB_URL / DATABASE_URL environment variable.",
            searched_path.display(),
            target,
        ),
        ConnectError::Config(ConfigError::ConnectionNotFound { name, available }) => format!(
            "Connection '{}' not found in {}.\n  \
             Available connections: {:?}\n  \
             Fix: pass --name <one-of-above>, or add a [[connections.{}]] section to the config.",
            name, cfg_display, available, name,
        ),
        ConnectError::Config(ConfigError::ConfigParse { path, source }) => format!(
            "Failed to parse config at {}: {}\n  \
             Fix: validate TOML syntax (missing quotes/brackets, stray characters).",
            path.display(),
            source,
        ),
        ConnectError::Config(ConfigError::Io(e)) => format!(
            "I/O error while reading {} (target: connection '{}'): {}",
            cfg_display,
            target,
            e,
        ),
        ConnectError::Config(ConfigError::Keyring { username, .. }) => format!(
            "Keyring lookup failed for user '{}' (connection '{}', config: {})\n  \
             Likely cause: password was stored via `gaussdb-mcp store-password` under service 'gaussdb-mcp',\n  \
             but the gaussdb library crate queries service 'gaussdb' — the two namespaces do not overlap.\n  \
             Upstream fix tracked at: https://github.com/c2j/rust-opengauss/issues/35\n  \
             Workaround until the fix lands: set `password = \"<plaintext>\"` in the config file\n  \
             (it will be migrated back to keyring automatically once upstream aligns).",
            username, target, cfg_display,
        ),
        ConnectError::Config(ConfigError::Generic(msg)) if msg.contains("keyring") => {
            let username = msg
                .split("user '")
                .nth(1)
                .and_then(|s| s.split('\'').next())
                .unwrap_or("<unknown>");
            format!(
                "Keyring lookup failed for user '{}' (connection '{}', config: {})\n  \
                 Likely cause: password was stored via `gaussdb-mcp store-password` under service 'gaussdb-mcp',\n  \
                 but the gaussdb library crate queries service 'gaussdb' — the two namespaces do not overlap.\n  \
                 Upstream fix tracked at: https://github.com/c2j/rust-opengauss/issues/35\n  \
                 Workaround until the fix lands: set `password = \"<plaintext>\"` in the config file\n  \
                 (it will be migrated back to keyring automatically once upstream aligns).",
                username, target, cfg_display,
            )
        }
        ConnectError::Config(ConfigError::Generic(msg)) => format!(
            "Config resolution failed for connection '{}' from {}: {}",
            target, cfg_display, msg,
        ),
        ConnectError::Tls(_) => format!(
            "TLS initialization failed for connection '{}' (config: {}).\n  \
             Fix: check the sslmode / certificate settings in the config.",
            target, cfg_display,
        ),
        ConnectError::TlsFeatureMissing { sslmode } => format!(
            "sslmode '{:?}' requires the 'tls-native-tls' feature, which is not enabled in this build of gaussdb.\n  \
             Fix: rebuild gaussdb with --features tls-native-tls, or set `sslmode = \"disable\"` in the config.",
            sslmode,
        ),
        ConnectError::Driver(driver_err) => {
            let mut chain: Vec<String> = Vec::new();
            let mut current: Option<&dyn std::error::Error> = Some(&**driver_err);
            while let Some(e) = current {
                let display = e.to_string();
                if !chain.contains(&display) {
                    chain.push(display);
                }
                current = e.source();
            }
            let root = chain.last().cloned().unwrap_or_else(|| chain[0].clone());
            format!(
                "Database connection failed for connection '{}' (config: {}).\n  \
                 Driver error: {}\n  \
                 Root cause: {}",
                target, cfg_display, driver_err, root,
            )
        }
    };

    anyhow::Error::new(err).context(context_msg)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Connection to a non-existent host must return an error (never panic).
    ///
    /// Uses a throwaway config file pointing at an unroutable host:port so the
    /// test does not depend on the global `~/.gaussdb-mcp.toml` and exercises
    /// the real `connect_sync` → `map_connect_error` path end-to-end.
    #[test]
    #[cfg(feature = "db")]
    fn test_connection_failure_returns_error() {
        let dir = std::env::temp_dir().join(format!("ogexplain_test_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let config_path = dir.join("bad.toml");
        std::fs::write(
            &config_path,
            "host = \"127.0.0.1\"\nport = 1\nuser = \"nobody\"\npassword = \"x\"\n\
             dbname = \"x\"\nsslmode = \"disable\"\n",
        )
        .unwrap();

        let result = fetch_explain(Some(&config_path), None, "SELECT 1", false);
        assert!(result.is_err(), "Expected connection error, got Ok");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Feature-gated path without `db` feature should bail gracefully.
    #[test]
    #[cfg(not(feature = "db"))]
    fn test_no_db_feature_bails() {
        let result = fetch_explain(None, None, "SELECT 1", false);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("not compiled"),
            "Expected feature-gate error, got: {msg}"
        );
    }

    #[test]
    #[cfg(feature = "db")]
    fn test_build_explain_sql_plain() {
        let sql = build_explain_sql("SELECT 1", false);
        assert_eq!(sql, "EXPLAIN SELECT 1");
    }

    #[test]
    #[cfg(feature = "db")]
    fn test_build_explain_sql_analyze() {
        let sql = build_explain_sql("SELECT * FROM t WHERE id = 1", true);
        assert_eq!(sql, "EXPLAIN ANALYZE SELECT * FROM t WHERE id = 1");
    }

    #[test]
    #[cfg(feature = "db")]
    fn test_build_explain_sql_complex_query() {
        let input =
            "SELECT a.x, b.y FROM a JOIN b ON a.id = b.id GROUP BY a.x ORDER BY b.y LIMIT 10";
        let sql = build_explain_sql(input, false);
        assert!(sql.starts_with("EXPLAIN "));
        assert!(sql.contains("GROUP BY"));
        assert!(sql.contains("ORDER BY"));
    }
}
