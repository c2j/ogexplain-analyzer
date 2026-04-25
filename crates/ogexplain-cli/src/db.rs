//! Database connection and EXPLAIN fetching.
//!
//! Connects to an OpenGauss/GaussDB instance, runs EXPLAIN (or EXPLAIN ANALYZE),
//! and returns the raw TEXT output suitable for feeding into `ogexplain_core::parse()`.

use anyhow::{Context, Result};

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

    let prefix = if analyze {
        "EXPLAIN ANALYZE"
    } else {
        "EXPLAIN"
    };
    let explain_sql = format!("{prefix} {sql}");

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
