use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::*;
use rmcp::{tool, tool_router, ErrorData};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

// ---- Server struct ----

#[derive(Debug, Clone, Default)]
pub struct OgexplainServer;

// ---- Tool parameter structs ----

#[derive(Debug, Deserialize, JsonSchema)]
pub struct AnalyzeExplainParams {
    /// The EXPLAIN or EXPLAIN ANALYZE output text to analyze
    pub explain_text: String,
    /// Optional original SQL text (enables SQL rewrite suggestions for correlated subqueries)
    #[serde(default)]
    pub sql_text: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ParseExplainParams {
    /// The EXPLAIN output text to parse into a structured plan tree
    pub explain_text: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetSuggestionsParams {
    /// The EXPLAIN output text to analyze for cross-rule optimization suggestions
    pub explain_text: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ScoreSqlComplexityParams {
    /// SQL statement to score for complexity
    pub sql_text: String,
}

// ---- Response types ----

#[derive(Debug, Serialize, JsonSchema)]
pub struct RuleInfo {
    pub id: String,
    pub name: String,
    pub category: String,
    pub description: String,
}

// ---- Tool implementations ----

#[tool_router(server_handler)]
impl OgexplainServer {
    #[tool(
        name = "analyze_explain",
        description = "Parse and analyze an OpenGauss EXPLAIN plan. Returns structured diagnostic findings with severity, rule IDs, suggestions, and a summary."
    )]
    async fn analyze_explain(
        &self,
        Parameters(params): Parameters<AnalyzeExplainParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let plan = ogexplain_core::parse(&params.explain_text)
            .map_err(|e| ErrorData::invalid_params(e.to_string(), None))?;

        let report = ogexplain_core::analyze_with_rewrite(&plan, params.sql_text.as_deref());

        let mut contents = Vec::new();

        // Structured JSON output
        contents.push(Content::json(&report)?);

        // Human-readable text summary
        let text_summary = format_text_summary(&report);
        contents.push(Content::text(text_summary));

        Ok(CallToolResult::success(contents))
    }

    // ---- Tool 2: Parse EXPLAIN text into structured plan tree ----

    #[tool(
        name = "parse_explain",
        description = "Parse EXPLAIN text into a structured plan tree with node types, costs, actual stats, and properties. Use this when you need to inspect the plan structure rather than run diagnostics."
    )]
    async fn parse_explain(
        &self,
        Parameters(params): Parameters<ParseExplainParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let plan = ogexplain_core::parse(&params.explain_text)
            .map_err(|e| ErrorData::invalid_params(e.to_string(), None))?;
        Ok(CallToolResult::success(vec![Content::json(&plan)?]))
    }

    // ---- Tool 3: List diagnostic rules ----

    #[tool(
        name = "list_diagnostic_rules",
        description = "List all available diagnostic rules with IDs, categories, and descriptions. Use this to understand what checks the analyzer performs."
    )]
    async fn list_diagnostic_rules(&self) -> Result<CallToolResult, ErrorData> {
        let rules = vec![
            RuleInfo {
                id: "SCAN-001".into(),
                name: "Large table full scan".into(),
                category: "scan".into(),
                description: "Sequential scans on tables exceeding row threshold".into(),
            },
            RuleInfo {
                id: "SCAN-004".into(),
                name: "Filter without index".into(),
                category: "scan".into(),
                description: "Filter removing many rows without index support".into(),
            },
            RuleInfo {
                id: "JOIN-001".into(),
                name: "Nested loop on large tables".into(),
                category: "join".into(),
                description: "Nested loop join with high row counts".into(),
            },
            RuleInfo {
                id: "JOIN-002".into(),
                name: "Hash join spill to disk".into(),
                category: "join".into(),
                description: "Hash join exceeding work_mem and spilling".into(),
            },
            RuleInfo {
                id: "MEM-001".into(),
                name: "Sort spill to disk".into(),
                category: "memory".into(),
                description: "External merge sort spilling to disk".into(),
            },
            RuleInfo {
                id: "MEM-004".into(),
                name: "High peak memory".into(),
                category: "memory".into(),
                description: "Highest-memory node in subtree".into(),
            },
            RuleInfo {
                id: "SORT-003".into(),
                name: "Duplicate sort".into(),
                category: "sort".into(),
                description: "Duplicate sort operations in plan".into(),
            },
            RuleInfo {
                id: "NET-001".into(),
                name: "Broadcast large data".into(),
                category: "network".into(),
                description: "Broadcasting excessive rows across datanodes".into(),
            },
            RuleInfo {
                id: "EST-001".into(),
                name: "Severe row estimation error".into(),
                category: "estimation".into(),
                description: "Actual rows far from optimizer estimate".into(),
            },
            RuleInfo {
                id: "EST-004".into(),
                name: "Nested loop from underestimation".into(),
                category: "estimation".into(),
                description: "Nested Loop caused by row underestimation".into(),
            },
            RuleInfo {
                id: "PUSH-001".into(),
                name: "Query not pushed down".into(),
                category: "pushdown".into(),
                description: "FQS failure — query not shipped to datanodes".into(),
            },
            RuleInfo {
                id: "PUSH-002".into(),
                name: "Multi-layer streaming".into(),
                category: "pushdown".into(),
                description: "Excessive streaming layers between datanodes".into(),
            },
            RuleInfo {
                id: "TYPE-001".into(),
                name: "Implicit type coercion".into(),
                category: "type_coercion".into(),
                description: "Hidden implicit type casts in conditions".into(),
            },
            RuleInfo {
                id: "TYPE-004".into(),
                name: "LIKE with leading wildcard".into(),
                category: "type_coercion".into(),
                description: "LIKE pattern starting with wildcard prevents index usage".into(),
            },
            RuleInfo {
                id: "VEC-001".into(),
                name: "Mixed row/vector engines".into(),
                category: "vectorization".into(),
                description: "Row and vector engine boundaries with adapter overhead".into(),
            },
            RuleInfo {
                id: "GEN-001".into(),
                name: "Plan too deep".into(),
                category: "general".into(),
                description: "Execution plan exceeds depth threshold".into(),
            },
            RuleInfo {
                id: "SUBQ-001".into(),
                name: "Subquery not pulled up".into(),
                category: "subquery".into(),
                description: "SubqueryScan nodes preventing optimization".into(),
            },
            RuleInfo {
                id: "REW-001".into(),
                name: "Large IN list not rewritten".into(),
                category: "subquery".into(),
                description: "IN lists with many values that should use EXISTS".into(),
            },
            RuleInfo {
                id: "SUBQ-006".into(),
                name: "Correlated subquery self-update".into(),
                category: "subquery".into(),
                description: "Self-referencing correlated subqueries in UPDATE/DELETE".into(),
            },
            RuleInfo {
                id: "AGG-001".into(),
                name: "Group aggregate should be hash".into(),
                category: "aggregate".into(),
                description: "Group Aggregate should use Hash Aggregate".into(),
            },
            RuleInfo {
                id: "AGG-002".into(),
                name: "Hash aggregate spill to disk".into(),
                category: "aggregate".into(),
                description: "Hash Aggregate exceeding work_mem".into(),
            },
            RuleInfo {
                id: "SKEW-001".into(),
                name: "Data skew detected".into(),
                category: "distribution".into(),
                description: "Uneven row distribution across datanodes".into(),
            },
            RuleInfo {
                id: "DIST-001".into(),
                name: "Distribution column mismatch".into(),
                category: "distribution".into(),
                description: "Join columns don't match distribution columns".into(),
            },
            RuleInfo {
                id: "STATS-001".into(),
                name: "Stats not collected".into(),
                category: "stats".into(),
                description: "Tables with missing or stale statistics".into(),
            },
            RuleInfo {
                id: "PART-001".into(),
                name: "Partition pruning failure".into(),
                category: "partition".into(),
                description: "Full partition scan when pruning should reduce".into(),
            },
        ];
        Ok(CallToolResult::success(vec![Content::json(&rules)?]))
    }

    // ---- Tool 4: Get cross-rule optimization suggestions ----

    #[tool(
        name = "get_suggestions",
        description = "Analyze EXPLAIN plan and return cross-rule optimization suggestions. Synthesizes multiple findings into higher-level recommendations (e.g., multiple spills → increase work_mem, scan+join → composite index)."
    )]
    async fn get_suggestions(
        &self,
        Parameters(params): Parameters<GetSuggestionsParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let plan = ogexplain_core::parse(&params.explain_text)
            .map_err(|e| ErrorData::invalid_params(e.to_string(), None))?;
        let report = ogexplain_core::analyze(&plan);
        let suggestions = ogexplain_core::suggester::SuggestionEngine::suggest(&report.findings);
        let mut contents = vec![Content::json(&suggestions)?];
        if suggestions.is_empty() {
            contents.push(Content::text(
                "No cross-rule synthesis suggestions. Individual findings may still have per-rule suggestions.",
            ));
        }
        Ok(CallToolResult::success(contents))
    }

    // ---- Tool 5: Score SQL complexity (standard + GaussDB) ----

    #[tool(
        name = "score_sql_complexity",
        description = "Score SQL statement complexity (0-100) with GaussDB-specific dimensions. Returns both standard and GaussDB complexity scores."
    )]
    async fn score_sql_complexity(
        &self,
        Parameters(params): Parameters<ScoreSqlComplexityParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let standard = ogsql_complexity::analyze(&params.sql_text)
            .map_err(|e| ErrorData::invalid_params(e.to_string(), None))?;
        let config = ogsql_complexity::ComplexityConfig::default();
        let gauss = ogsql_complexity::gauss_analyze(&params.sql_text, &config)
            .map_err(|e| ErrorData::invalid_params(e.to_string(), None))?;
        Ok(CallToolResult::success(vec![
            Content::json(&standard)?,
            Content::json(&gauss)?,
        ]))
    }
}

// ---- Helper functions ----

fn format_text_summary(report: &ogexplain_core::analyzer::report::DiagnosticReport) -> String {
    use ogexplain_core::analyzer::report::Severity;
    let mut lines = Vec::new();
    let findings = &report.findings;

    if findings.is_empty() {
        return "No diagnostic issues found. The execution plan looks healthy.".to_string();
    }

    lines.push(format!("Found {} diagnostic issue(s):\n", findings.len()));

    for f in findings {
        let severity_icon = match f.severity {
            Severity::Critical => "CRITICAL",
            Severity::Warning => "WARNING",
            Severity::Info => "INFO",
        };
        lines.push(format!("[{}] {} - {}", severity_icon, f.rule_id, f.title));
        if let Some(ref suggestion) = f.suggestion {
            lines.push(format!("  Suggestion: {}", suggestion));
        }
    }

    lines.join("\n")
}

pub fn run() {
    let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
    rt.block_on(async {
        let server = OgexplainServer;
        let transport = rmcp::transport::io::stdio();
        match rmcp::serve_server(server, transport).await {
            Ok(service) => {
                let _ = service.waiting().await;
            }
            Err(e) => {
                eprintln!("MCP server error: {e}");
                std::process::exit(1);
            }
        }
    });
}
