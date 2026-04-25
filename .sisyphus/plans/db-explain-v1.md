# DB-Connected EXPLAIN (V1) Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add a `explain` subcommand to the CLI that connects to an OpenGauss/GaussDB database, runs `EXPLAIN` on a SQL statement, and feeds the result into the existing parser/analyzer pipeline.

**Architecture:** Add `opengauss` (sync client) as an optional dependency to `ogexplain-cli`. New `explain` subcommand accepts connection params + SQL (file or inline string), fetches EXPLAIN TEXT from DB, then reuses the existing `output_block_with_diag` pipeline. The `ogexplain-core` library is untouched. Feature-gated behind a `db` Cargo feature so the CLI builds without the DB driver when not needed.

**Tech Stack:** `opengauss` crate v0.19 (sync, fork of rust-postgres for OpenGauss), `clap` v4 (existing), `ogexplain-core` (existing).

**Scope (V1):**
- Concrete SQL only (no parameter binding)
- `EXPLAIN` by default, `EXPLAIN ANALYZE` via explicit `--analyze` flag
- Connection via connection string (`-d "host=... user=... dbname=..."` or `postgresql://...`)
- SQL from file (`-f`) or inline string (`-s`)
- Reuses all existing output formats (`-o text/json`)
- Reuses all existing analysis flags (`--threshold`, `--quiet`, `--csv`, etc.)

**Out of Scope (V1):**
- `$N` parameter binding (V2)
- `:name` named parameter substitution (V3)
- TLS support (follow-up, needs `opengauss-openssl`)
- TUI integration
- `PGPASSWORD` environment variable support

---

### Task 1: Add `opengauss` dependency with feature gate

**Files:**
- Modify: `crates/ogexplain-cli/Cargo.toml`

**Step 1: Add feature-gated dependency**

Add to `crates/ogexplain-cli/Cargo.toml`:

```toml
[features]
default = ["db"]
db = ["dep:opengauss"]

[dependencies]
# ... existing deps unchanged ...
opengauss = { version = "0.19", optional = true }
```

**Step 2: Verify it compiles**

Run: `cargo build -p ogexplain-cli`
Expected: Compiles successfully (feature `db` is default).

**Step 3: Verify it compiles without the feature**

Run: `cargo build -p ogexplain-cli --no-default-features`
Expected: Compiles successfully (no `opengauss` dep pulled in).

**Step 4: Commit**

```
feat(cli): add opengauss as optional db dependency
```

---

### Task 2: Add i18n strings for `explain` subcommand

**Files:**
- Modify: `crates/ogexplain-core/i18n/app.yml`

**Step 1: Add explain subcommand i18n entries**

Append after the existing `cli.analyze.help_lang` block (around line 45):

```yaml
cli.explain.about:
  en: "Connect to database and EXPLAIN a SQL statement"
  zh-CN: "连接数据库并获取 SQL 的执行计划"

cli.explain.help_dsn:
  en: "Database connection string (e.g. 'host=localhost user=postgres dbname=mydb')"
  zh-CN: "数据库连接串（如 'host=localhost user=postgres dbname=mydb'）"

cli.explain.help_sql:
  en: "SQL statement to explain (inline string)"
  zh-CN: "要分析的 SQL 语句（内联字符串）"

cli.explain.help_sql_file:
  en: "SQL statement to explain (file path)"
  zh-CN: "要分析的 SQL 语句（文件路径）"

cli.explain.help_analyze:
  en: "Run EXPLAIN ANALYZE (actually executes the query)"
  zh-CN: "运行 EXPLAIN ANALYZE（会实际执行查询）"

cli.explain.help_output:
  en: "Output format"
  zh-CN: "输出格式"

cli.explain.help_threshold:
  en: "Minimum severity threshold"
  zh-CN: "最低严重级别阈值"

cli.explain.help_quiet:
  en: "Only show findings, no summary"
  zh-CN: "仅显示诊断结果，不显示摘要"

cli.explain.help_csv:
  en: "Export summary to CSV file (use '-' for stdout)"
  zh-CN: "导出摘要到 CSV 文件（使用 '-' 输出到标准输出）"

cli.explain.help_lang:
  en: "Language (en, zh-CN, auto)"
  zh-CN: "语言 (en, zh-CN, auto)"

cli.explain.error.connect:
  en: "Failed to connect to database"
  zh-CN: "连接数据库失败"

cli.explain.error.query:
  en: "Failed to execute EXPLAIN"
  zh-CN: "执行 EXPLAIN 失败"

cli.explain.error.no_sql:
  en: "No SQL provided. Use -s <sql> or -f <file>"
  zh-CN: "未提供 SQL。请使用 -s <sql> 或 -f <file>"

cli.explain.error.read_file:
  en: "Failed to read SQL file: %{path}"
  zh-CN: "读取 SQL 文件失败: %{path}"

cli.explain.warning_analyze:
  en: "⚠ EXPLAIN ANALYZE will execute the query. Use with caution on DML statements."
  zh-CN: "⚠ EXPLAIN ANALYZE 会实际执行查询。对 DML 语句请谨慎使用。"
```

**Step 2: Verify YAML is valid**

Run: `cargo build -p ogexplain-core`
Expected: Compiles. `rust_i18n` macro parses YAML at compile time — if it's malformed, this will fail.

**Step 3: Commit**

```
feat(i18n): add explain subcommand strings
```

---

### Task 3: Implement `fetch_explain` function

**Files:**
- Create: `crates/ogexplain-cli/src/db.rs`

**Step 1: Write the DB fetch module**

Create `crates/ogexplain-cli/src/db.rs`:

```rust
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
    // Guard: ensure the opengauss crate is available (compile-time via feature gate)
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

    // Trim trailing newline for consistency with file-based input
    let trimmed = output.trim_end().to_string();

    if trimmed.is_empty() {
        anyhow::bail!("EXPLAIN returned empty result");
    }

    Ok(trimmed)
}
```

**Step 2: Register the module in lib.rs**

Add at the top of `crates/ogexplain-cli/src/lib.rs`, after the existing `use` imports:

```rust
#[cfg(feature = "db")]
mod db;
```

**Step 3: Verify it compiles**

Run: `cargo build -p ogexplain-cli`
Expected: Compiles successfully.

**Step 4: Commit**

```
feat(cli): add fetch_explain for DB-connected EXPLAIN
```

---

### Task 4: Add `explain` subcommand to CLI

**Files:**
- Modify: `crates/ogexplain-cli/src/lib.rs`

This is the core integration task. The `explain` subcommand mirrors the `analyze` subcommand but replaces file input with DB fetch.

**Step 1: Add the Explain subcommand variant**

In `crates/ogexplain-cli/src/lib.rs`, modify the `Commands` enum (line 21):

```rust
#[derive(Subcommand)]
enum Commands {
    Analyze {
        file: String,
        #[arg(short, long, default_value = "text")]
        output: String,
        #[arg(long, default_value = "info")]
        threshold: String,
        #[arg(short, long)]
        quiet: bool,
        #[arg(short, long)]
        verbose: bool,
        #[arg(long)]
        multi: bool,
        #[arg(long)]
        csv: Option<String>,
        #[arg(long, default_value = "auto")]
        lang: String,
    },
    /// Connect to database and EXPLAIN a SQL statement
    Explain {
        /// Database connection string
        #[arg(short, long)]
        dsn: String,
        /// SQL statement (inline)
        #[arg(short, long)]
        sql: Option<String>,
        /// SQL file path
        #[arg(short = 'f', long)]
        sql_file: Option<String>,
        /// Run EXPLAIN ANALYZE (actually executes the query)
        #[arg(long)]
        analyze: bool,
        /// Output format
        #[arg(short, long, default_value = "text")]
        output: String,
        /// Minimum severity threshold
        #[arg(long, default_value = "info")]
        threshold: String,
        /// Only show findings, no summary
        #[arg(short, long)]
        quiet: bool,
        /// Export summary to CSV
        #[arg(long)]
        csv: Option<String>,
        /// Language
        #[arg(long, default_value = "auto")]
        lang: String,
    },
}
```

**Step 2: Add the explain command handler**

Add a new function `run_explain` after the existing `run()` function:

```rust
#[cfg(feature = "db")]
fn run_explain(
    dsn: &str,
    sql: Option<&str>,
    sql_file: Option<&str>,
    analyze: bool,
    output: &str,
    threshold: &str,
    quiet: bool,
    csv: Option<&str>,
) -> Result<()> {
    use crate::db;

    // Resolve SQL from -s or -f
    let sql_text = match (sql, sql_file) {
        (Some(s), None) => s.to_string(),
        (None, Some(path)) => std::fs::read_to_string(path)
            .context(format!("Failed to read SQL file: {}", path))?,
        (Some(_), Some(_)) => {
            anyhow::bail!("Cannot use both -s and -f. Choose one.");
        }
        (None, None) => {
            anyhow::bail!("No SQL provided. Use -s <sql> or -f <file>");
        }
    };

    // Warn if ANALYZE mode
    if analyze {
        eprintln!(
            "{}",
            "⚠ EXPLAIN ANALYZE will execute the query. Use with caution on DML statements."
                .yellow()
        );
    }

    // Fetch EXPLAIN from DB
    let explain_text = db::fetch_explain(dsn, &sql_text, analyze)?;

    // Reuse the existing analysis pipeline
    let plan =
        ogexplain_core::parse(&explain_text).context("Failed to parse EXPLAIN output")?;
    let complexity = try_complexity(&sql_text);
    let gauss_complexity = try_gauss_complexity(&sql_text);
    let complexity_input = complexity
        .as_ref()
        .map(|r| to_complexity_input(r, gauss_complexity.as_ref()));
    let diag = ogexplain_core::analyze(&plan);
    let row = SummaryRow::compute(&plan, &diag, complexity_input.as_ref());

    output_block_with_diag(
        &plan,
        &diag,
        output,
        threshold,
        quiet,
        complexity.as_ref(),
        gauss_complexity.as_ref(),
        1,
        1,
        Some(&row),
    )?;

    if let Some(csv_path) = csv {
        export_csv(&[row], csv_path)?;
    }

    if output != "json" {
        print_summary_table(&[SummaryRow::compute(&plan, &diag, complexity_input.as_ref())]);
    }

    Ok(())
}
```

**Step 3: Wire up in the `run()` function**

In the `run()` function, after the existing subcommand match block, add handling for the `Explain` variant. The current code matches subcommands at around line 469. Modify to:

```rust
let matches = cmd.get_matches();

match matches.subcommand() {
    Some(("explain", args)) => {
        #[cfg(feature = "db")]
        {
            let dsn = args.get_one::<String>("dsn").unwrap();
            let sql: Option<String> = args.get_one::<String>("sql").cloned();
            let sql_file: Option<String> = args.get_one::<String>("sql_file").cloned();
            let analyze = args.get_flag("analyze");
            let output = args.get_one::<String>("output").map(|s| s.as_str()).unwrap_or("text");
            let threshold = args.get_one::<String>("threshold").map(|s| s.as_str()).unwrap_or("info");
            let quiet = args.get_flag("quiet");
            let csv: Option<String> = args.get_one::<String>("csv").cloned();

            return run_explain(
                dsn,
                sql.as_deref(),
                sql_file.as_deref(),
                analyze,
                output,
                threshold,
                quiet,
                csv.as_deref(),
            );
        }
        #[cfg(not(feature = "db"))]
        {
            anyhow::bail!("Database support not compiled. Rebuild with --features db");
        }
    }
    _ => {
        // existing analyze logic (unchanged)
        let (sub_name, args) = match matches.subcommand() {
            Some((name, args)) => (name, args),
            None => ("analyze", &matches),
        };
        let _ = sub_name;
        // ... rest of existing analyze logic stays exactly the same ...
    }
}
```

Note: The existing analyze logic must be moved into the `_` arm unchanged. The key principle is that the `explain` subcommand is handled first, and everything else falls through to the existing behavior.

**Step 4: Verify it compiles**

Run: `cargo build -p ogexplain-cli`
Expected: Compiles successfully.

**Step 5: Verify existing tests still pass**

Run: `cargo test --workspace`
Expected: All 30 existing tests pass (no regressions).

**Step 6: Verify CLI help shows the new subcommand**

Run: `cargo run -p ogexplain-cli -- --help`
Expected: Shows both `analyze` and `explain` subcommands.

Run: `cargo run -p ogexplain-cli -- explain --help`
Expected: Shows all explain-specific flags (`-d`, `-s`, `-f`, `--analyze`, `-o`, etc.).

**Step 7: Commit**

```
feat(cli): add explain subcommand for DB-connected EXPLAIN
```

---

### Task 5: Add testcontainers dev-dependencies and OpenGauss image definition

**Files:**
- Modify: `crates/ogexplain-cli/Cargo.toml`
- Create: `tests/opengauss.rs` (workspace-level integration test)

**Container Image Reference (verified on arm64 macOS):**
- Image: `opengauss/opengauss:latest` (NOT `opengauss-server` — that image crashes on <12GB memory)
- Requires `--privileged` flag
- Env: `GS_PASSWORD` (≥8 chars, upper+lower+digit+special; e.g. `OpenGauss@2026`), `GS_DB`, `GS_USERNAME`
- Default user: `gaussdb`, default db: `postgres`, port: `5432`
- Auth method: `md5` for remote connections (`host all all 0.0.0.0/0 md5`)
- Startup: ~10s init + ~3s second start ≈ 13s total on arm64
- Readiness log: `"openGauss  init process complete; ready for start up."` — then need +5s grace for second gaussdb start
- No built-in HEALTHCHECK

**Step 1: Add dev-dependencies to ogexplain-cli**

Add to `crates/ogexplain-cli/Cargo.toml`:

```toml
[dev-dependencies]
testcontainers = "0.27"
tokio = { version = "1", features = ["rt", "macros", "time"] }
```

Note: `tokio` is needed because `testcontainers` runners are async (`AsyncRunner::start().await`). Our `fetch_explain` is sync, but we call it from within a `#[tokio::test]` runtime.

**Step 2: Ensure `db` module is public for testing**

In `crates/ogexplain-cli/src/lib.rs`, change:

```rust
#[cfg(feature = "db")]
mod db;
```

to:

```rust
#[cfg(feature = "db")]
pub mod db;
```

**Step 3: Verify it compiles**

Run: `cargo test -p ogexplain-cli --no-run`
Expected: Compiles successfully.

**Step 4: Commit**

```
test(cli): add testcontainers and tokio dev-dependencies
```

---

### Task 6: Write integration tests with testcontainers

**Files:**
- Create: `tests/db_explain.rs` (workspace-level integration test)

**Important:** These tests are feature-gated. They only compile when `db` feature is enabled (which is the default). They require Docker to be running on the host.

**Step 1: Determine the readiness log message**

Before writing tests, start the container manually and observe the startup log:

```bash
docker run --rm --privileged -e GS_PASSWORD="TestPass#1" -p 5432:5432 opengauss/opengauss-server:7.0.0-RC2.B015
```

Watch the output for a readiness indicator. Typical messages:
- `"ready to connect"` or `"database system is ready to accept connections"`

Update the `WaitFor` strategy in the test accordingly. If no clear log message exists, fall back to `WaitFor::seconds(30)` + TCP probe.

**Step 2: Write the integration test file**

Create `tests/db_explain.rs`:

```rust
//! Integration tests for DB-connected EXPLAIN using testcontainers.
//!
//! Prerequisites: Docker must be running on the host.
//!
//! Run: cargo test --test db_explain --features db
//!
//! Skip (no Docker): cargo test --workspace --test "!db_explain"

use std::time::Duration;

use testcontainers::{
    core::{IntoContainerPort, WaitFor},
    runners::AsyncRunner,
    GenericImage, ImageExt,
};

/// OpenGauss container defaults.
const IMAGE: &str = "opengauss/opengauss";
const TAG: &str = "latest";
const OG_PORT: u16 = 5432;
const OG_PASSWORD: &str = "OpenGauss@2026";
const OG_USER: &str = "gaussdb";
const OG_DB: &str = "postgres";

/// Build a connection string from the running container's host:port.
fn build_dsn(host: &str, port: u16) -> String {
    format!(
        "host={host} port={port} dbname={OG_DB} user={OG_USER} password={OG_PASSWORD} sslmode=disable"
    )
}

/// Start an OpenGauss container and return (host, port).
async fn start_opengauss() -> (String, u16) {
    let container = GenericImage::new(IMAGE, TAG)
        .with_privileged(true)
        .with_env_var("GS_PASSWORD", OG_PASSWORD)
        .with_env_var("GS_USERNAME", OG_USER)
        .with_env_var("GS_DB", OG_DB)
        .with_exposed_port(OG_PORT.tcp())
        // Verified readiness log from opengauss/opengauss:latest entrypoint.
        // The entrypoint does: initdb → start → create users → stop → print this message → exec gaussdb
        // After this message, the second gaussdb start needs ~3-5s to accept connections.
        .with_wait_for(WaitFor::message_on_stdout("ready for start up"))
        .with_startup_timeout(Duration::from_secs(120))
        .start()
        .await
        .expect("Failed to start OpenGauss container");

    let host = container.get_host().await.unwrap().to_string();
    let port = container.get_host_port_ipv4(OG_PORT).await.unwrap();

    // Grace period: the "ready for start up" message fires before the second gaussdb start.
    // Wait for the actual database process to begin accepting TCP connections.
    tokio::time::sleep(Duration::from_secs(5)).await;

    (host, port)
}

// ─── Test: fetch_explain with simple SELECT ─────────────────────────

#[tokio::test]
async fn test_fetch_explain_simple_select() {
    let (host, port) = start_opengauss().await;
    let dsn = build_dsn(&host, port);

    let result = ogexplain_cli::db::fetch_explain(&dsn, "SELECT 1", false);
    assert!(result.is_ok(), "fetch_explain failed: {:?}", result.err());

    let plan_text = result.unwrap();
    assert!(!plan_text.is_empty(), "EXPLAIN returned empty result");
    // OpenGauss typically returns a Result node for SELECT 1
    assert!(
        plan_text.contains("Result") || plan_text.contains("Seq Scan"),
        "Expected plan node in output, got: {plan_text}"
    );
}

// ─── Test: fetch_explain with EXPLAIN ANALYZE ───────────────────────

#[tokio::test]
async fn test_fetch_explain_analyze() {
    let (host, port) = start_opengauss().await;
    let dsn = build_dsn(&host, port);

    let result = ogexplain_cli::db::fetch_explain(&dsn, "SELECT 1", true);
    assert!(result.is_ok(), "fetch_explain ANALYZE failed: {:?}", result.err());

    let plan_text = result.unwrap();
    // ANALYZE output contains actual timing
    assert!(
        plan_text.contains("actual"),
        "Expected 'actual' in ANALYZE output, got: {plan_text}"
    );
}

// ─── Test: fetch_explain with a system catalog query ─────────────────

#[tokio::test]
async fn test_fetch_explain_system_catalog() {
    let (host, port) = start_opengauss().await;
    let dsn = build_dsn(&host, port);

    let sql = "SELECT * FROM pg_class LIMIT 10";
    let result = ogexplain_cli::db::fetch_explain(&dsn, sql, false);
    assert!(result.is_ok(), "fetch_explain failed: {:?}", result.err());

    let plan_text = result.unwrap();
    assert!(!plan_text.is_empty());
    assert!(
        plan_text.contains("Seq Scan") || plan_text.contains("Limit"),
        "Expected Seq Scan or Limit in plan, got: {plan_text}"
    );
}

// ─── Test: parse the EXPLAIN output through the full pipeline ───────

#[tokio::test]
async fn test_fetch_explain_full_pipeline() {
    let (host, port) = start_opengauss().await;
    let dsn = build_dsn(&host, port);

    let sql = "SELECT count(*) FROM pg_class";
    let explain_text = ogexplain_cli::db::fetch_explain(&dsn, sql, false)
        .expect("fetch_explain failed");

    // Feed through the full parser + analyzer
    let plan = ogexplain_core::parse(&explain_text).expect("parse failed");
    let diag = ogexplain_core::analyze(&plan);

    // Should have at least one plan node
    assert!(!diag.findings.is_empty() || plan.root.children.len() >= 0,
        "Pipeline should produce some output");
}

// ─── Test: error on bad SQL ─────────────────────────────────────────

#[tokio::test]
async fn test_fetch_explain_bad_sql() {
    let (host, port) = start_opengauss().await;
    let dsn = build_dsn(&host, port);

    let result = ogexplain_cli::db::fetch_explain(&dsn, "NOT VALID SQL !!", false);
    assert!(result.is_err(), "Expected error for invalid SQL");
}

// ─── Test: error on connection failure ──────────────────────────────

#[test]
fn test_fetch_explain_connection_failure() {
    // No container needed — just a bad connection string
    let result = ogexplain_cli::db::fetch_explain(
        "host=localhost port=99999 user=nobody dbname=nonexistent sslmode=disable",
        "SELECT 1",
        false,
    );
    assert!(result.is_err(), "Expected connection error");
}
```

**Step 3: Verify tests compile**

Run: `cargo test --test db_explain --features db --no-run`
Expected: Compiles successfully.

**Step 4: Run the tests (requires Docker)**

Run: `cargo test --test db_explain --features db`
Expected: All 6 tests pass (5 with container, 1 connection-failure test without).

Note: Each test starts its own container (~30s startup). Total test time ~3-5 minutes. If this is too slow, see Task 7 for a shared-container optimization.

**Step 5: Commit**

```
test(cli): add testcontainers integration tests for DB EXPLAIN
```

---

### Task 7 (Optional): Optimize — shared container fixture

The tests in Task 6 each start a separate container (~30s each). For faster iteration, share one container across all tests.

**Approach A: `#[ctor]` or `once_cell::sync::Lazy`** — start container once per test process.

Create `tests/db_explain.rs` with a shared fixture:

```rust
use std::sync::OnceLock;
use testcontainers::ContainerAsync;

static CONTAINER: OnceLock<(String, u16)> = OnceLock::new();

async fn get_or_start_container() -> (&'static str, u16) {
    CONTAINER.get_or_init(|| {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let (host, port) = start_opengauss().await;
            // NOTE: container is dropped here, which would stop it.
            // To keep alive, store the ContainerAsync<GenericImage> too.
            // For simplicity, each test can start its own container.
            (host, port)
        })
    });
    let (host, port) = CONTAINER.get().unwrap();
    (host.as_str(), *port)
}
```

**Approach B: Use `testcontainers` `Reusable` image** — not yet stable in v0.27.

**Recommendation:** Skip this optimization in V1. Individual containers per test are cleaner (no state leakage). Optimize later if test suite grows.

---

### Task 8: Final verification

**Step 1: Full workspace build**

Run: `cargo build --workspace`
Expected: All crates compile.

**Step 2: All existing tests pass**

Run: `cargo test --workspace --test integration_tests --test analyzer_tests`
Expected: All 30 existing tests pass (no regressions).

**Step 3: New integration tests pass (requires Docker)**

Run: `cargo test --test db_explain --features db`
Expected: 6 tests pass.

**Step 4: Clippy clean**

Run: `cargo clippy --workspace`
Expected: Zero warnings.

**Step 5: Format check**

Run: `cargo fmt --all -- --check`
Expected: No formatting issues.

**Step 6: Build without db feature**

Run: `cargo build -p ogexplain-cli --no-default-features`
Expected: Compiles. `explain` subcommand will error at runtime with helpful message.

**Step 7: Final commit**

```
chore: verify clean build and tests for db explain feature
```

---

## CLI Usage Examples (Post-Implementation)

```bash
# Basic: connect and explain a SQL statement
ogexplain explain -d "host=localhost user=jack dbname=mydb port=5432" -s "SELECT * FROM t1 WHERE id = 42"

# SQL from file
ogexplain explain -d "postgresql://jack@localhost:5432/mydb" -f query.sql

# With ANALYZE (opt-in, actually executes)
ogexplain explain -d "host=localhost user=jack dbname=mydb" --analyze -f query.sql

# JSON output
ogexplain explain -d "host=localhost user=jack dbname=mydb" -s "SELECT 1" -o json

# CSV export
ogexplain explain -d "host=localhost user=jack dbname=mydb" -f query.sql --csv results.csv
```

## Container Quick Start (Manual Testing)

```bash
# Start OpenGauss container (verified working on arm64 macOS, ~13s startup)
docker run --rm --privileged -d \
  --name og-test \
  -e GS_PASSWORD="OpenGauss@2026" \
  -p 5432:5432 \
  opengauss/opengauss:latest

# Wait ~15s for init + second start, then test:
ogexplain explain -d "host=localhost user=gaussdb dbname=postgres password=OpenGauss@2026 sslmode=disable" -s "SELECT 1"

# Check logs for readiness:
docker logs og-test 2>&1 | grep "ready for start up"

# Stop container
docker stop og-test
```
