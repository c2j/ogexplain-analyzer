//! Database connection and EXPLAIN fetching.
//!
//! Connects to an OpenGauss/GaussDB instance, runs EXPLAIN (or EXPLAIN ANALYZE),
//! and returns the raw TEXT output suitable for feeding into `ogexplain_core::parse()`.
//!
//! # TLS support
//!
//! By default connections use plaintext (`NoTls`), matching `sslmode=disable`.
//! For TLS-encrypted connections, enable the `db-tls` Cargo feature:
//!
//! ```sh
//! cargo build -p ogexplain-cli --features db-tls
//! ```
//!
//! When `db-tls` is enabled, the connector honors the DSN `sslmode` parameter:
//! - `disable` (or absent) → `NoTls` (plaintext, no encryption)
//! - `require` / `allow` / `prefer` → TLS without certificate verification
//! - `verify-ca` / `verify-full` → TLS with full certificate verification
//!
//! When `db-tls` is NOT enabled and the DSN requests TLS (non-disable sslmode),
//! an actionable error is returned instead of silently failing the connection.

use anyhow::{Context, Result};

/// Build the EXPLAIN SQL statement from a user SQL and analyze flag.
///
/// Visible for testing so we can verify the SQL construction without a database.
#[cfg(feature = "db")]
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
/// * `dsn` - Connection string (key-value or postgresql:// URL)
/// * `sql` - The SQL statement to explain (must be concrete, no $N params)
/// * `analyze` - If true, run EXPLAIN ANALYZE instead of EXPLAIN
///
/// # Errors
/// Returns error if connection fails or EXPLAIN execution fails.
pub fn fetch_explain(dsn: &str, sql: &str, analyze: bool) -> Result<String> {
    #[cfg(not(feature = "db"))]
    {
        let _ = (dsn, sql, analyze);
        anyhow::bail!("Database support not compiled. Rebuild with --features db");
    }

    #[cfg(feature = "db")]
    {
        fetch_explain_impl(dsn, sql, analyze)
    }
}

#[cfg(feature = "db")]
fn fetch_explain_impl(dsn: &str, sql: &str, analyze: bool) -> Result<String> {
    use gaussdb::SimpleQueryMessage;

    let sslmode = parse_sslmode(dsn);
    let mut client = connect(dsn, sslmode)?;

    let explain_sql = build_explain_sql(sql, analyze);

    let messages = client
        .simple_query(&explain_sql)
        .context("Failed to execute EXPLAIN")?;

    let mut output = String::new();
    for msg in &messages {
        if let SimpleQueryMessage::Row(row) = msg {
            if let Some(text) = row.get(0) {
                output.push_str(text);
                output.push('\n');
            }
        }
    }

    let trimmed = output.trim_end().to_string();

    if trimmed.is_empty() {
        anyhow::bail!("EXPLAIN returned empty result");
    }

    Ok(trimmed)
}

/// Extract the `sslmode` value from a libpq-style or URL connection string.
///
/// Returns an empty slice if `sslmode` is not present.
#[cfg(feature = "db")]
fn parse_sslmode(dsn: &str) -> &str {
    const MARKER: &str = "sslmode=";
    let Some(start) = dsn.find(MARKER) else {
        return "";
    };
    let value_start = start + MARKER.len();
    let rest = &dsn[value_start..];
    let value_end = rest
        .find(|c: char| c.is_whitespace() || c == '&')
        .unwrap_or(rest.len());
    rest[..value_end].trim_matches(|c| c == '\'' || c == '"')
}

/// Connect to the database, selecting `NoTls` or TLS based on the `sslmode` value.
#[cfg(feature = "db")]
fn connect(dsn: &str, sslmode: &str) -> Result<gaussdb::sync::Client> {
    use gaussdb::sync::NoTls;

    if matches!(sslmode, "" | "disable") {
        gaussdb::sync::Client::connect(dsn, NoTls).context("Failed to connect to database")
    } else {
        #[cfg(feature = "db-tls")]
        {
            connect_tls(dsn, sslmode)
        }
        #[cfg(not(feature = "db-tls"))]
        {
            let _ = dsn;
            anyhow::bail!(
                "Connection requires TLS (sslmode='{sslmode}') but the 'db-tls' feature \
                 is not enabled. Rebuild with: cargo build -p ogexplain-cli --features db-tls"
            );
        }
    }
}

/// Connect with native-tls, choosing verification strictness from `sslmode`.
///
/// Per libpq semantics:
/// - `verify-ca`: verify CA chain but NOT hostname
/// - `verify-full`: verify CA chain AND hostname
#[cfg(feature = "db-tls")]
fn connect_tls(dsn: &str, sslmode: &str) -> Result<gaussdb::sync::Client> {
    use gaussdb::native_tls::MakeTlsConnector;

    let verify_cert = matches!(sslmode, "verify-ca" | "verify-full");
    let verify_hostname = sslmode == "verify-full";
    let mut builder = native_tls::TlsConnector::builder();
    if !verify_cert {
        builder.danger_accept_invalid_certs(true);
    }
    if !verify_hostname {
        builder.danger_accept_invalid_hostnames(true);
    }
    let connector = builder
        .build()
        .context("Failed to build native-tls connector")?;
    let tls = MakeTlsConnector::new(connector);
    gaussdb::sync::Client::connect(dsn, tls).context("Failed to connect to database")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Connection to a non-existent host must return an error (never panic).
    #[test]
    #[cfg(feature = "db")]
    fn test_connection_failure_returns_error() {
        let result = fetch_explain(
            "host=localhost port=99999 user=nobody dbname=nonexistent sslmode=disable",
            "SELECT 1",
            false,
        );
        assert!(result.is_err(), "Expected connection error, got Ok");
    }

    /// Feature-gated path without `db` feature should bail gracefully.
    #[test]
    #[cfg(not(feature = "db"))]
    fn test_no_db_feature_bails() {
        let result = fetch_explain("host=x", "SELECT 1", false);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("not compiled"),
            "Expected feature-gate error, got: {msg}"
        );
    }

    /// Verify EXPLAIN SQL construction (no database needed).
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

    #[test]
    #[cfg(feature = "db")]
    fn test_parse_sslmode_libpq_disable() {
        assert_eq!(parse_sslmode("host=localhost sslmode=disable"), "disable");
    }

    #[test]
    #[cfg(feature = "db")]
    fn test_parse_sslmode_libpq_middle() {
        assert_eq!(parse_sslmode("host=x sslmode=require user=y"), "require");
    }

    #[test]
    #[cfg(feature = "db")]
    fn test_parse_sslmode_libpq_first() {
        assert_eq!(parse_sslmode("sslmode=verify-full host=x"), "verify-full");
    }

    #[test]
    #[cfg(feature = "db")]
    fn test_parse_sslmode_absent() {
        assert_eq!(parse_sslmode("host=localhost user=postgres dbname=mydb"), "");
    }

    #[test]
    #[cfg(feature = "db")]
    fn test_parse_sslmode_url_query() {
        assert_eq!(
            parse_sslmode("postgresql://user@host:5432/db?sslmode=require"),
            "require"
        );
    }

    #[test]
    #[cfg(feature = "db")]
    fn test_parse_sslmode_quoted() {
        assert_eq!(parse_sslmode("sslmode='require' host=x"), "require");
    }
}
