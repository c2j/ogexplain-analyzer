rust_i18n::i18n!("i18n", fallback = "en");

pub mod analyzer;
pub mod i18n;
pub mod model;
pub mod parser;
pub mod rewriter;
pub mod sql;
pub mod suggester;
pub mod summary;

pub use parser::parse;
pub use parser::parse_multi;

pub fn analyze(plan: &model::ExplainPlan) -> analyzer::report::DiagnosticReport {
    analyzer::DiagnosticEngine::new(analyzer::config::DiagnosticConfig::default()).analyze(plan)
}

pub fn analyze_with_config(
    plan: &model::ExplainPlan,
    config: &analyzer::config::DiagnosticConfig,
) -> analyzer::report::DiagnosticReport {
    analyzer::DiagnosticEngine::new(config.clone()).analyze(plan)
}

pub fn heatmap(plan: &model::ExplainPlan) -> Option<analyzer::heatmap::PlanHeatmap> {
    analyzer::heatmap::HeatmapEngine::generate(plan)
}

/// Generate a resource waterfall for the plan.
///
/// Returns None if the plan has no EXPLAIN ANALYZE data.
pub fn waterfall(plan: &model::ExplainPlan) -> Option<analyzer::waterfall::PlanWaterfall> {
    analyzer::waterfall::WaterfallEngine::generate(plan)
}

pub fn analyze_with_rewrite(
    plan: &model::ExplainPlan,
    sql_text: Option<&str>,
) -> analyzer::report::DiagnosticReport {
    analyze_with_rewrite_and_config(plan, sql_text, &analyzer::config::DiagnosticConfig::default())
}

pub fn analyze_with_rewrite_and_config(
    plan: &model::ExplainPlan,
    sql_text: Option<&str>,
    config: &analyzer::config::DiagnosticConfig,
) -> analyzer::report::DiagnosticReport {
    let mut report = analyze_with_config(plan, config);

    if let Some(sql) = sql_text {
        let (stmts, errors) = ogsql_parser::parser::Parser::parse_sql(sql);
        if errors.is_empty() {
            if let Some(info) = stmts.into_iter().next() {
                if let Ok(Some(_)) =
                    rewriter::detector::detect_correlated_subquery_update(&info.statement)
                {
                    if let Ok(result) = rewriter::transform::rewrite_update_from(&info.statement) {
                        for finding in &mut report.findings {
                            if finding.rule_id == "SUBQ-006" {
                                finding.sql_rewrite = Some(result.clone());
                            }
                        }
                    }
                }
            }
        }
    }

    report
}
