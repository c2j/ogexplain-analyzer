//! Database connection and EXPLAIN fetching.
//!
//! Connects to an OpenGauss/GaussDB instance, runs EXPLAIN (or EXPLAIN ANALYZE),
//! and returns the raw TEXT output suitable for feeding into `ogexplain_core::parse()`.

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
    use opengauss::{Client, NoTls, SimpleQueryMessage};

    let mut client = Client::connect(dsn, NoTls).context("Failed to connect to database")?;

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
        let input = "SELECT a.x, b.y FROM a JOIN b ON a.id = b.id GROUP BY a.x ORDER BY b.y LIMIT 10";
        let sql = build_explain_sql(input, false);
        assert!(sql.starts_with("EXPLAIN "));
        assert!(sql.contains("GROUP BY"));
        assert!(sql.contains("ORDER BY"));
    }
}
