//! Anti-pattern subtree matching module.
//!
//! This module implements **structural anti-pattern detection**: matching
//! known-bad plan tree shapes via recursive DFS and producing diagnostics
//! with structured evidence.
//!
//! Architecture:
//! - [`types`] — core data structures (`MatchResult`, `Evidence`, `MatchedNode`)
//! - [`engine`] — [`AntiPatternDef`] trait + [`PatternEngine`] DFS walker
//! - [`predicates`] — [`FieldAccessor`] for unified node field extraction
//! - [`templates`] — simple `{capture.property}` string rendering
//! - [`patterns`] — individual anti-pattern implementations
//!
//! Integration point: [`AntiPatternRule`] implements [`DiagnosticRule`] and
//! is registered in [`all_rules`](crate::analyzer::rules::all_rules).

pub mod engine;
pub mod patterns;
pub mod predicates;
pub mod templates;
pub mod types;

use crate::analyzer::context::GlobalStats;
use crate::analyzer::report::{DiagnosticCategory, Finding, Severity};
use crate::analyzer::rules::DiagnosticRule;
use crate::model::{ExplainPlan, PlanNode};
use rust_i18n::t;

use engine::{AntiPatternDef, PatternEngine};
use types::{Evidence, MatchResult, MatchedNode};

/// A single [`DiagnosticRule`] that runs all registered anti-patterns
/// via the [`PatternEngine`] in its [`check_global`](DiagnosticRule::check_global) hook.
///
/// Anti-patterns are not evaluated during per-node traversal
/// ([`check`](DiagnosticRule::check) always returns `None`).
pub struct AntiPatternRule {
    engine: PatternEngine,
}

impl AntiPatternRule {
    /// Create a new rule with all anti-patterns registered.
    pub fn new() -> Self {
        let patterns: Vec<Box<dyn AntiPatternDef>> = vec![
            Box::new(patterns::materialize_cascade::MaterializeCascade),
            Box::new(patterns::index_scan_amplify::IndexScanAmplify::default()),
            Box::new(patterns::gather_then_sort::GatherThenSort::default()),
            Box::new(patterns::nested_loop_sort::NestedLoopSort::default()),
            Box::new(patterns::hash_join_skewed::HashJoinSkewed::default()),
            Box::new(patterns::multi_distinct::MultiDistinct),
            Box::new(patterns::index_heap_fetches::IndexHeapFetches::default()),
            Box::new(patterns::agg_over_streaming::AggOverStreaming::default()),
        ];
        Self {
            engine: PatternEngine::new(patterns),
        }
    }

    /// Convert a [`MatchResult`] into a [`Finding`] by rendering templates
    /// and building evidence.
    fn render_finding(&self, result: MatchResult<'_>) -> Finding {
        let pattern = self.engine.find_pattern(&result.pattern_id);
        let detail = templates::render_detail(pattern, &result);
        let suggestion = templates::render_suggestion(pattern, &result);

        Finding {
            rule_id: result.pattern_id.clone(),
            severity: pattern.severity(),
            category: pattern.category(),
            title: pattern.name().to_string(),
            detail,
            node_line: Some(result.matched_node.line_number),
            node_type: Some(result.matched_node.node_type.to_string()),
            suggestion: Some(suggestion),
            sql_rewrite: None,
            evidence: Some(Evidence {
                pattern_id: result.pattern_id,
                confidence: 1.0_f64,
                matched_nodes: result
                    .captures
                    .iter()
                    .map(|(name, node)| MatchedNode {
                        capture_name: name.clone(),
                        line_number: node.line_number,
                        node_type: node.node_type.to_string(),
                        relation: node.relation.clone(),
                    })
                    .collect::<Vec<_>>(),
                related_classic_rules: pattern.related_classic_rules(),
            }),
            table: None,
            columns: Vec::new(),
        }
    }
}

impl Default for AntiPatternRule {
    fn default() -> Self {
        Self::new()
    }
}

impl DiagnosticRule for AntiPatternRule {
    fn id(&self) -> &str {
        "ANTI"
    }

    fn name(&self) -> String {
        t!("finding.ANTI.name").to_string()
    }

    fn severity(&self) -> Severity {
        Severity::Warning
    }

    fn category(&self) -> DiagnosticCategory {
        DiagnosticCategory::General
    }

    /// Anti-patterns are not evaluated per-node — returns `None`.
    fn check(
        &self,
        _node: &PlanNode,
        _ctx: &crate::analyzer::context::PlanContext,
    ) -> Option<Finding> {
        None
    }

    /// Run all anti-patterns against the entire plan tree via the engine.
    fn check_global(&self, plan: &ExplainPlan, _stats: &GlobalStats) -> Vec<Finding> {
        // Use disabled_rules compatible approach:
        // each Finding's rule_id is the anti-pattern ID (e.g. "ANTI-005"),
        // so DiagnosticEngine's `findings.retain()` will filter correctly.
        self.engine
            .match_plan(plan)
            .into_iter()
            .map(|result| self.render_finding(result))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_anti_pattern_rule_id() {
        let rule = AntiPatternRule::new();
        assert_eq!(rule.id(), "ANTI");
    }

    #[test]
    fn test_anti_pattern_rule_check_returns_none() {
        let rule = AntiPatternRule::new();
        let node = PlanNode {
            node_type: crate::model::NodeType::SeqScan,
            relation: None,
            join_type: None,
            estimated: None,
            actual: None,
            properties: vec![],
            structured_props: None,
            buffers: None,
            children: vec![],
            indent_level: 0,
            line_number: 1,
        };
        let plan = ExplainPlan {
            root: node,
            summary: None,
        };
        // We need a minimal PlanContext; uses global_stats & plan refs
        let stats = GlobalStats::compute(&plan);
        let ctx = crate::analyzer::context::PlanContext {
            plan: &plan,
            global_stats: &stats,
        };
        assert!(rule.check(&plan.root, &ctx).is_none());
    }

    #[test]
    fn test_anti_pattern_rule_check_global_empty() {
        let rule = AntiPatternRule::new();
        let node = PlanNode {
            node_type: crate::model::NodeType::SeqScan,
            relation: None,
            join_type: None,
            estimated: None,
            actual: None,
            properties: vec![],
            structured_props: None,
            buffers: None,
            children: vec![],
            indent_level: 0,
            line_number: 1,
        };
        let plan = ExplainPlan {
            root: node,
            summary: None,
        };
        let stats = GlobalStats::compute(&plan);
        let findings = rule.check_global(&plan, &stats);
        assert!(findings.is_empty());
    }
}
