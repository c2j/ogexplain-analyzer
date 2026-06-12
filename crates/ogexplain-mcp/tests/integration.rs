use ogexplain_mcp::server::OgexplainServer;
use rmcp::model::CallToolRequestParams;

/// EXPLAIN ANALYZE plan with high estimation error (estimated 100000 vs actual 500).
/// This triggers SCAN-004 (Filter without index) in the analyzer.
const EXPLAIN_FIXTURE: &str = "Seq Scan on orders  (cost=0.00..25000.00 rows=100000 width=68) (actual time=0.034..234.567 rows=500 loops=1)\n  Filter: (status = 42)\n  Rows Removed by Filter: 500000\n";

/// A minimal EXPLAIN plan (no actual stats) — parses cleanly, no diagnostics.
const SIMPLE_EXPLAIN: &str = "Seq Scan on dual  (cost=0.00..2.00 rows=1 width=1)\n";

// ---------------------------------------------------------------------------
// Helper: create a connected server–client pair and return the client peer.
// ---------------------------------------------------------------------------
async fn create_client() -> rmcp::service::RunningService<rmcp::RoleClient, ()> {
    let (server_io, client_io) = tokio::io::duplex(8192);

    // Start the MCP server in a background task.
    tokio::spawn(async move {
        let server = OgexplainServer::default();
        let running = rmcp::serve_server(server, server_io).await;
        match running {
            Ok(r) => {
                let _ = r.waiting().await;
            }
            Err(e) => {
                panic!("MCP server failed to start: {e}");
            }
        }
    });

    // Connect a client to the same transport.
    rmcp::serve_client((), client_io)
        .await
        .expect("MCP client should connect to server")
}

/// Helper: make a tool call and unwrap, panicking on is_error.
async fn call_tool(
    client: &rmcp::service::RunningService<rmcp::RoleClient, ()>,
    name: &str,
    explain_text: &str,
) -> rmcp::model::CallToolResult {
    let mut args = serde_json::Map::new();
    args.insert(
        "explain_text".to_string(),
        serde_json::Value::String(explain_text.to_owned()),
    );

    let result = client
        .call_tool(CallToolRequestParams::new(name.to_owned()).with_arguments(args))
        .await
        .expect("tool call should succeed at transport level");

    // Fail fast if the tool itself returned an error.
    assert!(
        result.is_error != Some(true),
        "tool '{name}' returned an error: {result:?}"
    );

    result
}

/// Helper: parse the first text content from a tool result as JSON.
fn first_content_as_json(result: &rmcp::model::CallToolResult) -> serde_json::Value {
    let c = &result.content[0];
    let text = c.as_text().expect("content[0] should be text");
    serde_json::from_str(&text.text).expect("text content should be valid JSON")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_list_tools_returns_all_5() {
    let client = create_client().await;

    let tools = client
        .list_all_tools()
        .await
        .expect("list_tools should succeed");

    assert_eq!(tools.len(), 5, "expected exactly 5 tools");

    let names: Vec<&str> = tools.iter().map(|t| t.name.as_ref()).collect();
    assert!(
        names.contains(&"analyze_explain"),
        "missing analyze_explain tool"
    );
    assert!(
        names.contains(&"parse_explain"),
        "missing parse_explain tool"
    );
    assert!(
        names.contains(&"list_diagnostic_rules"),
        "missing list_diagnostic_rules tool"
    );
    assert!(
        names.contains(&"get_suggestions"),
        "missing get_suggestions tool"
    );
    assert!(
        names.contains(&"score_sql_complexity"),
        "missing score_sql_complexity tool"
    );
}

#[tokio::test]
async fn test_analyze_explain_returns_findings() {
    let client = create_client().await;
    let result = call_tool(&client, "analyze_explain", EXPLAIN_FIXTURE).await;

    // analyze_explain returns two content items: JSON + text summary.
    assert!(
        result.content.len() >= 2,
        "expected at least 2 content items, got {}",
        result.content.len()
    );

    // Verify JSON content has findings.
    let json = first_content_as_json(&result);
    let findings = json
        .get("findings")
        .and_then(|f| f.as_array())
        .expect("JSON should contain 'findings' array");

    // The SCAN-004 rule should fire for the filter without index.
    let rule_ids: Vec<&str> = findings
        .iter()
        .filter_map(|f| f.get("rule_id").and_then(|r| r.as_str()))
        .collect();
    assert!(
        rule_ids.contains(&"SCAN-004"),
        "expected SCAN-004 finding, got: {rule_ids:?}"
    );

    // Verify text summary is present.
    let summary_text = &result.content[1];
    let text = summary_text.as_text().expect("content[1] should be text");
    assert!(!text.text.is_empty(), "text summary should not be empty");
    assert!(
        text.text.contains("SCAN-004"),
        "text summary should mention SCAN-004"
    );
}

#[tokio::test]
async fn test_parse_explain_returns_plan_tree() {
    let client = create_client().await;
    let result = call_tool(&client, "parse_explain", SIMPLE_EXPLAIN).await;

    // parse_explain returns a single JSON content item.
    assert!(
        !result.content.is_empty(),
        "expected at least 1 content item"
    );

    let json = first_content_as_json(&result);

    // ogexplain_core returns `ExplainPlan` with a `root` field containing the plan tree.
    let root = json
        .get("root")
        .expect("parsed plan should have a 'root' key");
    assert!(
        root.get("node_type").is_some(),
        "root node should contain 'node_type', got: {root}"
    );
    assert!(
        root.get("relation").is_some(),
        "root node should contain 'relation', got: {root}"
    );
}

#[tokio::test]
async fn test_list_diagnostic_rules_returns_25() {
    let client = create_client().await;

    // list_diagnostic_rules takes no arguments.
    let result = client
        .call_tool(CallToolRequestParams::new("list_diagnostic_rules"))
        .await
        .expect("list_diagnostic_rules should succeed");

    assert!(
        result.is_error != Some(true),
        "list_diagnostic_rules returned error"
    );
    assert!(
        !result.content.is_empty(),
        "expected at least 1 content item"
    );

    let json = first_content_as_json(&result);
    let rules = json
        .as_array()
        .expect("expected JSON array of diagnostic rules");

    assert_eq!(rules.len(), 25, "expected exactly 25 diagnostic rules");

    // Verify each rule has required fields.
    for (i, rule) in rules.iter().enumerate() {
        assert!(
            rule.get("id").and_then(|v| v.as_str()).is_some(),
            "rule[{i}] missing 'id'"
        );
        assert!(
            rule.get("name").and_then(|v| v.as_str()).is_some(),
            "rule[{i}] missing 'name'"
        );
        assert!(
            rule.get("category").and_then(|v| v.as_str()).is_some(),
            "rule[{i}] missing 'category'"
        );
    }

    // Verify specific rules are present.
    let ids: Vec<&str> = rules
        .iter()
        .filter_map(|r| r.get("id").and_then(|v| v.as_str()))
        .collect();
    assert!(
        ids.contains(&"SCAN-001"),
        "missing SCAN-001 in rules: {ids:?}"
    );
    assert!(
        ids.contains(&"SCAN-004"),
        "missing SCAN-004 in rules: {ids:?}"
    );
    assert!(
        ids.contains(&"JOIN-001"),
        "missing JOIN-001 in rules: {ids:?}"
    );
    assert!(
        ids.contains(&"PART-001"),
        "missing PART-001 in rules: {ids:?}"
    );
}
