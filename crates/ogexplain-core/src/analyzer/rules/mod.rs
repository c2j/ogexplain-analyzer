mod estimation_rules;
mod general_rules;
mod join_rules;
mod memory_rules;
mod network_rules;
mod pushdown_rules;
mod scan_rules;
mod sort_rules;
mod subquery_rules;
mod type_coercion_rules;
pub mod utils;
mod vectorization_rules;

use super::config::DiagnosticConfig;
use super::context::GlobalStats;
use super::report::{DiagnosticCategory, Finding, Severity};
use crate::model::{ExplainPlan, PlanNode};

pub trait DiagnosticRule: Send + Sync {
    fn id(&self) -> &str;
    fn name(&self) -> &str;
    fn severity(&self) -> Severity;
    fn category(&self) -> DiagnosticCategory;
    fn check(&self, node: &PlanNode, ctx: &super::context::PlanContext) -> Option<Finding>;
    /// Context-aware check with ancestor chain (root → ... → direct parent).
    /// Default: delegates to `check`, ignoring ancestors.
    /// Override when a rule needs parent context (e.g., "am I under a HashJoin?").
    fn check_with_ancestors(
        &self,
        node: &PlanNode,
        ctx: &super::context::PlanContext,
        ancestors: &[&PlanNode],
    ) -> Option<Finding> {
        let _ = ancestors;
        self.check(node, ctx)
    }
    fn check_global(&self, _plan: &ExplainPlan, _stats: &GlobalStats) -> Vec<Finding> {
        Vec::new()
    }
}

pub fn all_rules(config: &DiagnosticConfig) -> Vec<Box<dyn DiagnosticRule>> {
    vec![
        Box::new(scan_rules::LargeTableFullScan::new(config.clone())),
        Box::new(scan_rules::FilterWithoutIndex::new(config.clone())),
        Box::new(join_rules::NestedLoopLargeDataset::new(config.clone())),
        Box::new(join_rules::HashSpillToDisk),
        Box::new(memory_rules::SortSpillToDisk),
        Box::new(memory_rules::HighPeakMemory::new(config.clone())),
        Box::new(sort_rules::DuplicateSort),
        Box::new(network_rules::BroadcastLargeTable::new(config.clone())),
        Box::new(estimation_rules::SevereRowUnderestimation::new(
            config.clone(),
        )),
        Box::new(estimation_rules::NestedLoopFromUnderestimation::new(
            config.clone(),
        )),
        Box::new(pushdown_rules::QueryNotPushedDown),
        Box::new(pushdown_rules::MultiLayerStreaming),
        Box::new(type_coercion_rules::SuspectedImplicitTypeCast),
        Box::new(type_coercion_rules::LikeWithLeadingWildcard),
        Box::new(vectorization_rules::MixedVectorRowEngines),
        Box::new(general_rules::PlanTooDeep::new(config.clone())),
        Box::new(subquery_rules::SubqueryNotPulledUp),
        Box::new(subquery_rules::LargeInListNotConverted::new()),
        Box::new(subquery_rules::CorrelatedSubquerySelfUpdate),
        Box::new(super::pattern::AntiPatternRule::new()),
    ]
}

fn make_finding(
    rule: &dyn DiagnosticRule,
    detail: String,
    node: &PlanNode,
    suggestion: Option<String>,
) -> Finding {
    Finding {
        rule_id: rule.id().to_string(),
        severity: rule.severity(),
        category: rule.category(),
        title: rule.name().to_string(),
        detail,
        node_line: Some(node.line_number),
        node_type: Some(node.node_type.to_string()),
        suggestion,
        sql_rewrite: None,
        evidence: None,
    }
}
