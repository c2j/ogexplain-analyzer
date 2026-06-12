use super::suggestion::{Suggestion, SuggestionCategory};
use crate::analyzer::report::Finding;

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
                message: "多个估算偏差表明统计信息可能过期，建议对所有涉及的表执行 ANALYZE"
                    .to_string(),
                confidence: 0.85,
            });
        }

        let spill_rules: Vec<String> = findings
            .iter()
            .filter(|f| f.rule_id == "MEM-001" || f.rule_id == "JOIN-002")
            .map(|f| f.rule_id.clone())
            .collect();
        if spill_rules.len() >= 2 {
            suggestions.push(Suggestion {
                related_rules: spill_rules.clone(),
                category: SuggestionCategory::ConfigurationTuning,
                message: format!(
                    "检测到 {} 处内存溢出到磁盘，建议增大 work_mem 以避免磁盘 I/O",
                    spill_rules.len()
                ),
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
                message:
                    "同时检测到扫描和连接问题，在连接列和过滤列上创建复合索引可能有助于提升性能"
                        .to_string(),
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
                message: "检测到下推问题，检查查询中的不可下推构造并考虑数据重分布".to_string(),
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
                message: "检测到关联子查询自引用UPDATE，建议改写为 UPDATE ... FROM 或 CTE 形式以避免逐行执行".to_string(),
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
                message: "多处类型不一致问题, 建议全面审查 WHERE/JOIN 条件中的数据类型匹配"
                    .to_string(),
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
                message: "检测到引擎切换, 建议统一使用行引擎或向量化引擎以消除 Adapter 开销"
                    .to_string(),
                confidence: 0.8,
            });
        }

        suggestions
    }
}
