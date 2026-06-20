use super::report::{DiagnosticReport, Finding};
use super::rules::DiagnosticRule;
use crate::model::ExplainPlan;

#[derive(Debug, Clone)]
pub struct DiagnosticConfig {
    pub large_table_rows: f64,
    pub memory_threshold_kb: f64,
    pub estimation_skew_factor: f64,
    pub nested_loop_inner_rows: f64,
    pub sort_time_ratio: f64,
    pub max_plan_depth: usize,
    pub disabled_rules: Vec<String>,
}

impl Default for DiagnosticConfig {
    fn default() -> Self {
        Self {
            large_table_rows: 10000.0,
            memory_threshold_kb: 102400.0,
            estimation_skew_factor: 100.0,
            nested_loop_inner_rows: 10000.0,
            sort_time_ratio: 0.3,
            max_plan_depth: 10,
            disabled_rules: Vec::new(),
        }
    }
}

pub struct DiagnosticEngine {
    config: DiagnosticConfig,
    rules: Vec<Box<dyn DiagnosticRule>>,
}

impl DiagnosticEngine {
    pub fn new(config: DiagnosticConfig) -> Self {
        let rules = super::rules::all_rules(&config);
        Self { config, rules }
    }

    pub fn analyze(&self, plan: &ExplainPlan) -> DiagnosticReport {
        let stats = super::context::GlobalStats::compute(plan);
        let ctx = super::context::PlanContext {
            plan,
            global_stats: &stats,
        };

        let mut findings = Vec::new();
        self.walk_node(&plan.root, &ctx, &mut findings, &mut Vec::new());

        for rule in &self.rules {
            findings.extend(rule.check_global(plan, &stats));
        }

        findings.retain(|f| !self.config.disabled_rules.contains(&f.rule_id));

        DiagnosticReport { findings, stats }
    }

    fn walk_node<'a>(
        &self,
        node: &'a crate::model::PlanNode,
        ctx: &super::context::PlanContext,
        findings: &mut Vec<Finding>,
        ancestors: &mut Vec<&'a crate::model::PlanNode>,
    ) {
        for rule in &self.rules {
            if let Some(finding) = rule.check_with_ancestors(node, ctx, ancestors) {
                findings.push(finding);
            }
        }
        ancestors.push(node);
        for child in &node.children {
            self.walk_node(child, ctx, findings, ancestors);
        }
        ancestors.pop();
    }
}
