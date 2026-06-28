use super::suggestion::{Suggestion, SuggestionCategory};
use crate::analyzer::report::Finding;
use rust_i18n::t;

pub struct SuggestionEngine;

impl SuggestionEngine {
    pub fn suggest(findings: &[Finding]) -> Vec<Suggestion> {
        let mut suggestions = Vec::new();

        let est_rules: Vec<String> = findings
            .iter()
            .filter(|f| f.rule_id.starts_with("EST-"))
            .map(|f| f.rule_id.clone())
            .collect();
        if est_rules.len() >= 2 {
            suggestions.push(Suggestion {
                related_rules: est_rules,
                category: SuggestionCategory::StatisticsUpdate,
                message: t!("finding.suggester.multi_estimation").to_string(),
                confidence: 0.85,
            });
        }

        let spill_rules: Vec<String> = findings
            .iter()
            .filter(|f| {
                matches!(
                    f.rule_id.as_str(),
                    "MEM-001" | "MEM-004" | "JOIN-002" | "AGG-002"
                )
            })
            .map(|f| f.rule_id.clone())
            .collect();
        if spill_rules.len() >= 2 {
            suggestions.push(Suggestion {
                related_rules: spill_rules.clone(),
                category: SuggestionCategory::ConfigurationTuning,
                message: t!("finding.suggester.multi_spill", count = spill_rules.len()).to_string(),
                confidence: 0.9,
            });
        }

        let scan_findings: Vec<&Finding> = findings
            .iter()
            .filter(|f| f.rule_id.starts_with("SCAN-"))
            .collect();
        let join_findings: Vec<&Finding> = findings
            .iter()
            .filter(|f| f.rule_id.starts_with("JOIN-"))
            .collect();
        if !scan_findings.is_empty() && !join_findings.is_empty() {
            suggestions.push(Suggestion {
                related_rules: scan_findings
                    .iter()
                    .chain(join_findings.iter())
                    .map(|f| f.rule_id.clone())
                    .collect(),
                category: SuggestionCategory::IndexOptimization,
                message: t!("finding.suggester.scan_and_join").to_string(),
                confidence: 0.8,
            });
        }

        let push_findings: Vec<&Finding> = findings
            .iter()
            .filter(|f| f.rule_id.starts_with("PUSH-"))
            .collect();
        if !push_findings.is_empty() {
            suggestions.push(Suggestion {
                related_rules: push_findings.iter().map(|f| f.rule_id.clone()).collect(),
                category: SuggestionCategory::DistributionOptimization,
                message: t!("finding.suggester.pushdown_issues").to_string(),
                confidence: 0.75,
            });
        }

        let subq_findings: Vec<&Finding> = findings
            .iter()
            .filter(|f| f.rule_id == "SUBQ-006")
            .collect();
        if !subq_findings.is_empty() {
            suggestions.push(Suggestion {
                related_rules: subq_findings.iter().map(|f| f.rule_id.clone()).collect(),
                category: SuggestionCategory::QueryRewrite,
                message: t!("finding.suggester.subquery_self_update").to_string(),
                confidence: 0.9,
            });
        }

        let type_findings: Vec<&Finding> = findings
            .iter()
            .filter(|f| f.rule_id.starts_with("TYPE-"))
            .collect();
        if type_findings.len() >= 2 {
            suggestions.push(Suggestion {
                related_rules: type_findings.iter().map(|f| f.rule_id.clone()).collect(),
                category: SuggestionCategory::QueryRewrite,
                message: t!("finding.suggester.type_inconsistencies").to_string(),
                confidence: 0.85,
            });
        }

        let vec_findings: Vec<&Finding> = findings
            .iter()
            .filter(|f| f.rule_id.starts_with("VEC-"))
            .collect();
        if !vec_findings.is_empty() {
            suggestions.push(Suggestion {
                related_rules: vec_findings.iter().map(|f| f.rule_id.clone()).collect(),
                category: SuggestionCategory::ConfigurationTuning,
                message: t!("finding.suggester.engine_switches").to_string(),
                confidence: 0.8,
            });
        }

        suggestions
    }
}
