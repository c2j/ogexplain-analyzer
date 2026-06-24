//! Integration tests for DB-connected EXPLAIN using testcontainers.
//!
//! Prerequisites: Docker must be running on the host.
//!
//! Run: cargo test --test db_explain --features ogexplain-cli/db

use std::net::TcpStream;
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

async fn start_opengauss() -> (testcontainers::ContainerAsync<GenericImage>, String, u16) {
    // GenericImage methods (with_exposed_port, with_wait_for) must be called first,
    // before ImageExt methods (with_privileged, with_env_var, etc.) which return ContainerRequest.
    let container = GenericImage::new(IMAGE, TAG)
        .with_exposed_port(OG_PORT.tcp())
        .with_wait_for(WaitFor::message_on_stdout("ready for start up"))
        .with_privileged(true)
        .with_env_var("GS_PASSWORD", OG_PASSWORD)
        .with_env_var("GS_USERNAME", OG_USER)
        .with_env_var("GS_DB", OG_DB)
        .with_startup_timeout(Duration::from_secs(120))
        .start()
        .await
        .expect("Failed to start OpenGauss container");

    let host = container.get_host().await.unwrap().to_string();
    let port = container.get_host_port_ipv4(OG_PORT).await.unwrap();

    // The "ready for start up" message fires after initdb + first start + user creation + stop.
    // A second gaussdb start (the actual daemon) needs additional time. Poll TCP until ready.
    let mut connected = false;
    for _ in 0..30 {
        if TcpStream::connect((&*host, port)).is_ok() {
            connected = true;
            break;
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    assert!(connected, "OpenGauss never became ready on {host}:{port}");

    // Port is open but DB may still be initializing internals (loading plugins, etc.)
    tokio::time::sleep(Duration::from_secs(3)).await;

    (container, host, port)
}

// ─── Test: fetch_explain with simple SELECT ─────────────────────────

// fetch_explain is synchronous but opengauss internally uses tokio::block_on,
// so it cannot be called inside a tokio async context. spawn_blocking runs it
// on a dedicated thread where no runtime is active.
macro_rules! fetch {
    ($dsn:expr, $sql:expr, $analyze:expr) => {
        tokio::task::spawn_blocking({
            let dsn = $dsn.to_string();
            let sql = $sql.to_string();
            move || ogexplain_cli::db::fetch_explain(Some(&dsn), None, None, &sql, $analyze)
        })
        .await
        .expect("spawn_blocking panicked")
    };
}

#[tokio::test]
async fn test_fetch_explain_simple_select() {
    let (_container, host, port) = start_opengauss().await;
    let dsn = build_dsn(&host, port);

    let result = fetch!(&dsn, "SELECT 1", false);
    assert!(result.is_ok(), "fetch_explain failed: {:?}", result.err());

    let plan_text = result.unwrap();
    assert!(!plan_text.is_empty(), "EXPLAIN returned empty result");
    assert!(
        plan_text.contains("Result") || plan_text.contains("Seq Scan"),
        "Expected plan node in output, got: {plan_text}"
    );
}

#[tokio::test]
async fn test_fetch_explain_analyze() {
    let (_container, host, port) = start_opengauss().await;
    let dsn = build_dsn(&host, port);

    let result = fetch!(&dsn, "SELECT 1", true);
    assert!(
        result.is_ok(),
        "fetch_explain ANALYZE failed: {:?}",
        result.err()
    );

    let plan_text = result.unwrap();
    assert!(
        plan_text.contains("actual"),
        "Expected 'actual' in ANALYZE output, got: {plan_text}"
    );
}

#[tokio::test]
async fn test_fetch_explain_system_catalog() {
    let (_container, host, port) = start_opengauss().await;
    let dsn = build_dsn(&host, port);

    let result = fetch!(&dsn, "SELECT * FROM pg_class LIMIT 10", false);
    assert!(result.is_ok(), "fetch_explain failed: {:?}", result.err());

    let plan_text = result.unwrap();
    assert!(!plan_text.is_empty());
    assert!(
        plan_text.contains("Seq Scan") || plan_text.contains("Limit"),
        "Expected Seq Scan or Limit in plan, got: {plan_text}"
    );
}

#[tokio::test]
async fn test_fetch_explain_full_pipeline() {
    let (_container, host, port) = start_opengauss().await;
    let dsn = build_dsn(&host, port);

    let explain_text =
        fetch!(&dsn, "SELECT count(*) FROM pg_class", false).expect("fetch_explain failed");

    let plan = ogexplain_core::parse(&explain_text).expect("parse failed");
    let _diag = ogexplain_core::analyze(&plan);

    assert!(
        !plan.root.node_type.to_string().is_empty(),
        "Pipeline should produce a valid plan"
    );
}

#[tokio::test]
async fn test_fetch_explain_bad_sql() {
    let (_container, host, port) = start_opengauss().await;
    let dsn = build_dsn(&host, port);

    let result = fetch!(&dsn, "NOT VALID SQL !!", false);
    assert!(result.is_err(), "Expected error for invalid SQL");
}

// ─── Test: error on connection failure (no container needed) ────────

#[test]
fn test_fetch_explain_connection_failure() {
    let result = ogexplain_cli::db::fetch_explain(
        Some("host=localhost port=99999 user=nobody dbname=nonexistent sslmode=disable"),
        None,
        None,
        "SELECT 1",
        false,
    );
    assert!(result.is_err(), "Expected connection error");
}
