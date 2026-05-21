use crate::model::{NodeType, PlanNode, StreamingType};

use super::super::context::PlanContext;
use super::super::report::{DiagnosticCategory, Finding, Severity};
use super::{make_finding, DiagnosticRule};

// SUBQ-001: Correlated subquery not pulled up
pub struct SubqueryNotPulledUp;

impl DiagnosticRule for SubqueryNotPulledUp {
    fn id(&self) -> &str {
        "SUBQ-001"
    }
    fn name(&self) -> &str {
        "关联子查询未提升"
    }
    fn severity(&self) -> Severity {
        Severity::Warning
    }
    fn category(&self) -> DiagnosticCategory {
        DiagnosticCategory::SubqueryStructure
    }
    fn check(&self, node: &PlanNode, _ctx: &PlanContext) -> Option<Finding> {
        // Detect SubqueryScan nodes (subqueries that weren't pulled up into joins)
        if node.node_type == NodeType::SubqueryScan
            || node.node_type == NodeType::VectorSubqueryScan
        {
            return Some(make_finding(
                self,
                "检测到未提升的子查询(SubqueryScan)".to_string(),
                node,
                Some("检测到未提升的子查询(SubqueryScan), 可能导致临时表和性能下降; 改写为JOIN: /*+ EXPAND_SUBQUERY */; 若为关联子查询, 使用 /*+ EXPAND_SUBLINK */; 考虑使用 /*+ USE_MAGIC_SET */ 优化关联子查询".to_string()),
            ));
        }

        // Detect Result nodes with SubPlan in properties
        if node.node_type == NodeType::Result || node.node_type == NodeType::VectorResult {
            let has_subplan = node.properties.iter().any(|p| p.value.contains("SubPlan"));
            if has_subplan {
                return Some(make_finding(
                    self,
                    "检测到未提升的子查询(SubPlan)".to_string(),
                    node,
                    Some("检测到未提升的子查询(SubqueryScan/SubPlan), 可能导致临时表和性能下降; 改写为JOIN: /*+ EXPAND_SUBQUERY */; 若为关联子查询, 使用 /*+ EXPAND_SUBLINK */; 考虑使用 /*+ USE_MAGIC_SET */ 优化关联子查询".to_string()),
                ));
            }
        }

        None
    }
}

// REW-001: Large IN list not converted to JOIN
pub struct LargeInListNotConverted;

impl DiagnosticRule for LargeInListNotConverted {
    fn id(&self) -> &str {
        "REW-001"
    }
    fn name(&self) -> &str {
        "大IN列表未转换"
    }
    fn severity(&self) -> Severity {
        Severity::Warning
    }
    fn category(&self) -> DiagnosticCategory {
        DiagnosticCategory::SubqueryStructure
    }
    fn check(&self, node: &PlanNode, _ctx: &PlanContext) -> Option<Finding> {
        // Find Filter property with long IN(...) list
        let filter_prop = node
            .properties
            .iter()
            .find(|p| p.label == "Filter" && p.value.contains("IN ("))?;

        let comma_count = filter_prop.value.matches(',').count();
        if comma_count <= 10 {
            return None;
        }

        Some(make_finding(
            self,
            format!("过滤条件含长IN列表({}+个值)", comma_count + 1),
            node,
            Some("过滤条件含长IN列表; 使用 /*+ INLIST_TO_JOIN */ 将IN列表转换为JOIN; 或改写为临时表JOIN: INSERT INTO temp VALUES (...); SELECT * FROM t JOIN temp ON t.col = temp.col".to_string()),
        ))
    }
}

pub struct CorrelatedSubquerySelfUpdate;

impl DiagnosticRule for CorrelatedSubquerySelfUpdate {
    fn id(&self) -> &str {
        "SUBQ-006"
    }

    fn name(&self) -> &str {
        "关联子查询自引用UPDATE"
    }

    fn severity(&self) -> Severity {
        Severity::Warning
    }

    fn category(&self) -> DiagnosticCategory {
        DiagnosticCategory::SubqueryStructure
    }

    fn check(&self, node: &PlanNode, _ctx: &PlanContext) -> Option<Finding> {
        let is_dml = matches!(
            node.node_type,
            NodeType::Update | NodeType::ModifyTable | NodeType::VectorUpdate
        );
        if !is_dml {
            return None;
        }

        let target_table = extract_target_table(node)?;
        let signals = collect_signals(node, &target_table);

        if !signals.has_subplan || !signals.same_table_scan {
            return None;
        }

        let mut detail = format!(
            "检测到关联子查询自引用UPDATE (表: {}), SubPlan存在: {}, 同表扫描: {}",
            target_table, signals.has_subplan, signals.same_table_scan
        );

        if signals.has_streaming {
            detail.push_str(", 分布式场景: 存在Streaming重分布, 可能导致跨DN数据传输");
        }

        let suggestion = build_rewrite_template(&target_table, &signals.correlation_column);

        Some(make_finding(self, detail, node, Some(suggestion)))
    }
}

struct Signals {
    has_subplan: bool,
    same_table_scan: bool,
    has_streaming: bool,
    correlation_column: Option<String>,
}

fn collect_signals(node: &PlanNode, target_table: &str) -> Signals {
    let mut signals = Signals {
        has_subplan: false,
        same_table_scan: false,
        has_streaming: false,
        correlation_column: None,
    };

    check_node_recursive(node, target_table, &mut signals);
    signals
}

fn check_node_recursive(node: &PlanNode, target_table: &str, signals: &mut Signals) {
    let has_subplan_prop = node
        .properties
        .iter()
        .any(|p| p.value.contains("SubPlan"));
    if has_subplan_prop {
        signals.has_subplan = true;
    }

    if is_scan_node(&node.node_type) {
        if let Some(ref rel) = node.relation {
            if table_name_match(rel, target_table) {
                signals.same_table_scan = true;
            }
        }
    }

    if let NodeType::Streaming(stype) = &node.node_type {
        if matches!(
            stype,
            StreamingType::Redistribute
                | StreamingType::Broadcast
                | StreamingType::SplitRedistribute
                | StreamingType::SplitBroadcast
                | StreamingType::PartRedistributePartBroadcast
        ) {
            signals.has_streaming = true;
        }
    }
    if signals.correlation_column.is_none() {
        if let Some(col) = extract_correlation_column(node) {
            signals.correlation_column = Some(col);
        }
    }

    for child in &node.children {
        check_node_recursive(child, target_table, signals);
    }
}

fn is_scan_node(nt: &NodeType) -> bool {
    matches!(
        nt,
        NodeType::SeqScan
            | NodeType::IndexScan
            | NodeType::IndexOnlyScan
            | NodeType::BitmapHeapScan
            | NodeType::CStoreScan
            | NodeType::CStoreIndexScan
            | NodeType::PartitionedSeqScan
            | NodeType::PartitionedIndexScan
            | NodeType::PartitionedBitmapHeapScan
    )
}

fn extract_target_table(node: &PlanNode) -> Option<String> {
    if let Some(ref rel) = node.relation {
        return Some(first_identifier(rel));
    }
    if let Some(child) = node.children.first() {
        if let Some(ref rel) = child.relation {
            return Some(first_identifier(rel));
        }
        if let Some(grandchild) = child.children.first() {
            if let Some(ref rel) = grandchild.relation {
                return Some(first_identifier(rel));
            }
        }
    }
    None
}

fn table_name_match(relation: &str, target: &str) -> bool {
    first_identifier(relation) == target
}

fn first_identifier(s: &str) -> String {
    s.split_whitespace()
        .next()
        .unwrap_or(s)
        .to_string()
}

fn extract_correlation_column(node: &PlanNode) -> Option<String> {
    let prop = node
        .properties
        .iter()
        .find(|p| p.label == "Index Cond")?;
    let value = &prop.value;
    let paren_content = extract_innermost_parens(value)?;
    let parts: Vec<&str> = paren_content.splitn(2, '=').collect();
    if parts.len() == 2 {
        let col_part = parts[0].trim();
        let col = col_part
            .split('.')
            .next_back()
            .unwrap_or(col_part)
            .trim()
            .to_string();
        if !col.is_empty() {
            return Some(col);
        }
    }
    None
}

fn extract_innermost_parens(s: &str) -> Option<String> {
    let start = s.rfind('(')?;
    let end = s.rfind(')')?;
    if end > start {
        Some(s[start + 1..end].to_string())
    } else {
        None
    }
}

fn build_rewrite_template(table: &str, correlation_col: &Option<String>) -> String {
    let col = correlation_col
        .as_deref()
        .unwrap_or("<关联列>");
    format!(
        "关联子查询自引用UPDATE存在逐行执行O(n²)风险; 建议改写:\n\
         方式一(UPDATE FROM): UPDATE {table} SET ... = t.new_val FROM (SELECT {col}, ... FROM {table}) t WHERE {table}.{col} = t.{col};\n\
         方式二(CTE): WITH new_vals AS (SELECT {col}, ... FROM {table}) UPDATE {table} SET ... = n.new_val FROM new_vals n WHERE {table}.{col} = n.{col};"
    )
}
