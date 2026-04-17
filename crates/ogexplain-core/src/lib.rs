pub mod analyzer;
pub mod model;
pub mod parser;
pub mod sql;
pub mod suggester;

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
