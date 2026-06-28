// Live-DB regression driver.
//
// Requires `--features live-db` and a Docker daemon. Spawns an OpenGauss
// container, initialises it with ogagila schema + data, then replays EXPLAIN
// queries against a real database.
//
// This file is the Phase 3a delivery: container management + schema
// loading + a smoke test.

use std::io::Write;
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

use testcontainers::{
    core::{IntoContainerPort, WaitFor},
    runners::AsyncRunner,
    GenericImage, ImageExt,
};

const IMAGE: &str = "opengauss/opengauss";
const TAG: &str = "latest";
const OG_PORT: u16 = 5432;
const OG_PASSWORD: &str = "OpenGauss@2026";
const OG_USER: &str = "gaussdb";
const OG_DEFAULT_DB: &str = "postgres";

fn write_temp_config(host: &str, port: u16, dbname: &str) -> PathBuf {
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    let path = std::env::temp_dir().join(format!("ogexplain-live-{pid}-{nanos}.toml"));
    let content = format!(
        "host = \"{host}\"\nport = {port}\nuser = \"{OG_USER}\"\npassword = \"{OG_PASSWORD}\"\ndbname = \"{dbname}\"\nsslmode = \"disable\"\n"
    );
    let mut file = std::fs::File::create(&path).expect("failed to create temp config");
    file.write_all(content.as_bytes()).expect("failed to write");
    path
}

fn cleanup_temp_config(path: &Path) {
    let _ = std::fs::remove_file(path);
}

async fn start_opengauss() -> (testcontainers::ContainerAsync<GenericImage>, String, u16) {
    let container = GenericImage::new(IMAGE, TAG)
        .with_exposed_port(OG_PORT.tcp())
        .with_wait_for(WaitFor::message_on_stdout("ready for start up"))
        .with_privileged(true)
        .with_env_var("GS_PASSWORD", OG_PASSWORD)
        .with_startup_timeout(Duration::from_secs(120))
        .start()
        .await
        .expect("Failed to start OpenGauss container");

    let host = container.get_host().await.unwrap().to_string();
    let port = container.get_host_port_ipv4(OG_PORT).await.unwrap();

    let mut connected = false;
    for _ in 0..30 {
        if TcpStream::connect((&*host, port)).is_ok() {
            connected = true;
            break;
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    assert!(connected, "OpenGauss never became ready on {host}:{port}");
    tokio::time::sleep(Duration::from_secs(3)).await;

    (container, host, port)
}

fn ogagila_sql_dir() -> PathBuf {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    PathBuf::from(&manifest).join("lib/ogagila/sqls")
}

#[cfg(feature = "live-db")]
fn load_ogagila_schema_sync(config_path: &Path, container_id: &str) {
    let sql_dir = ogagila_sql_dir();
    let ddl_files: [(&str, &str); 5] = [
        ("1-ddl", "ddl/schema.sql"),
        ("2-ddl-jsonb", "ddl/schema-jsonb.sql"),
        ("3-functions", "program/functions.sql"),
        ("4-triggers", "program/triggers.sql"),
        ("5-views", "program/views.sql"),
    ];
    let data_files: [(&str, &str); 3] = [
        ("6-data", "init_data/data.sql"),
        ("7-apt-jsonb", "init_data/data-apt-jsonb.sql"),
        ("8-yum-jsonb", "init_data/data-yum-jsonb.sql"),
    ];

    let mut client = gaussdb::config::connect_sync(None, Some(config_path), None)
        .unwrap_or_else(|e| panic!("connect to ogagila: {e:?}"));

    for (name, rel) in &ddl_files {
        let path = sql_dir.join(rel);
        let sql = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {rel}: {e}"));
        eprintln!("loading {name} ({rel}, {} bytes)", sql.len());
        client
            .batch_execute(&sql)
            .unwrap_or_else(|e| panic!("{name} ({rel}) failed: {e}"));
    }

    for (name, rel) in &data_files {
        let path = sql_dir.join(rel);
        let sql = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {rel}: {e}"));
        eprintln!(
            "loading {name} ({rel}) via docker exec -i gsql ({} bytes)",
            sql.len()
        );
        let mut child = std::process::Command::new("docker")
            .args([
                "exec",
                "-i",
                container_id,
                "bash",
                "-c",
                "su - omm -c 'gsql -d pagila'",
            ])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap_or_else(|e| panic!("spawn docker exec gsql for {name}: {e}"));
        {
            let stdin = child.stdin.as_mut().expect("failed to open stdin");
            std::io::Write::write_all(stdin, sql.as_bytes())
                .unwrap_or_else(|e| panic!("write stdin for {name}: {e}"));
        }
        let output = child
            .wait_with_output()
            .unwrap_or_else(|e| panic!("wait docker exec for {name}: {e}"));
        if !output.status.success() {
            panic!(
                "{name} ({rel}) gsql failed (exit {}):\nstderr:\n{}\nstdout:\n{}",
                output.status,
                String::from_utf8_lossy(&output.stderr),
                String::from_utf8_lossy(&output.stdout),
            );
        }
    }
    eprintln!("ogagila schema loaded successfully");
}

#[tokio::test]
#[cfg(feature = "live-db")]
async fn smoke_start_and_explain() {
    let (container, host, port) = start_opengauss().await;
    let container_id = container.id().to_string();

    let host2 = host.clone();
    let pagila_config = tokio::task::spawn_blocking(move || -> PathBuf {
        let postgres_cfg = write_temp_config(&host2, port, OG_DEFAULT_DB);
        let mut client = gaussdb::config::connect_sync(None, Some(&postgres_cfg), None)
            .expect("connect to postgres");
        client
            .batch_execute("CREATE DATABASE pagila")
            .expect("CREATE DATABASE pagila");
        eprintln!("pagila database created");
        cleanup_temp_config(&postgres_cfg);

        let pagila_cfg = write_temp_config(&host2, port, "pagila");
        load_ogagila_schema_sync(&pagila_cfg, &container_id);
        pagila_cfg
    })
    .await
    .expect("spawn_blocking panicked");

    let plan = tokio::task::spawn_blocking({
        let cfg = pagila_config.clone();
        move || {
            ogexplain_cli::db::fetch_explain(
                Some(&cfg),
                None,
                "SELECT * FROM rental LIMIT 10",
                false,
            )
        }
    })
    .await
    .expect("spawn_blocking panicked")
    .expect("EXPLAIN query");

    assert!(
        plan.contains("Seq Scan") || plan.contains("Limit"),
        "Expected Seq Scan or Limit in plan, got: {plan}"
    );

    println!("smoke_pass: rental query returned EXPLAIN with plan nodes");
    println!("\n--- live EXPLAIN output ---\n{plan}\n--- end ---");
    cleanup_temp_config(&pagila_config);
}

#[test]
#[cfg(not(feature = "live-db"))]
fn live_db_not_enabled() {
    eprintln!("live-db feature not enabled — skipping live DB tests. Re-run with: cargo test --test regress_live --features live-db");
}
