use regex::Regex;
use std::sync::LazyLock;

static RE_COST_INFO: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"\(cost=([0-9]*\.?[0-9]+)\.\.([0-9]*\.?[0-9]+) rows=([0-9]*\.?[0-9]+) width=(\d+)\)",
    )
    .expect("failed to compile cost regex")
});

static RE_ACTUAL_TIME: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"actual time=([0-9]*\.?[0-9]+)\.\.([0-9]*\.?[0-9]+) rows=([0-9]*\.?[0-9]+) loops=([0-9]*\.?[0-9]+)")
        .expect("failed to compile actual time regex")
});

static RE_ACTUAL_ROWS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"actual rows=([0-9]*\.?[0-9]+) loops=([0-9]*\.?[0-9]+)")
        .expect("failed to compile actual rows regex")
});

static RE_NEVER_EXECUTED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"Actual time: never executed").expect("failed to compile never executed regex")
});

static RE_UNKNOWN_ACTUAL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"Actual time: unknown").expect("failed to compile unknown actual regex")
});

static RE_PRED_INFO: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"p-time=([0-9]*\.?[0-9]+) p-rows=([0-9]*\.?[0-9]+)")
        .expect("failed to compile pred info regex")
});

static RE_COST_WILDCARD: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\(cost=\.\*[^)]*\)").expect("failed to compile cost wildcard regex")
});

#[derive(Debug, Clone)]
pub enum ClassifiedLine {
    Node(NodeLine),
    Property(PropertyLine),
    Summary(SummaryLine),
    Separator,
    Blank,
    Header,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct NodeLine {
    pub indent: usize,
    pub indent_level: usize,
    pub is_child: bool,
    pub raw_node_text: String,
    pub cost: Option<CostInfo>,
    pub actual: Option<ActualInfo>,
    pub line_number: usize,
}

#[derive(Debug, Clone)]
pub struct CostInfo {
    pub startup_cost: f64,
    pub total_cost: f64,
    pub plan_rows: f64,
    pub plan_width: i32,
    pub pred_time: Option<f64>,
    pub pred_rows: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct ActualInfo {
    pub startup_time_ms: Option<f64>,
    pub total_time_ms: Option<f64>,
    pub rows: f64,
    pub loops: f64,
    pub executed: bool,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct PropertyLine {
    pub indent: usize,
    pub label: String,
    pub value: String,
    pub line_number: usize,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct SummaryLine {
    pub entries: Vec<(String, String)>,
    pub line_number: usize,
}

pub fn classify_lines(text: &str) -> Vec<ClassifiedLine> {
    let mut results = Vec::new();

    for (idx, line) in text.lines().enumerate() {
        let line_num = idx + 1;

        let working_line = if let Some(rest) = line.strip_prefix("--?") {
            rest
        } else {
            line
        };
        let trimmed = working_line.trim();

        if trimmed.is_empty() {
            results.push(ClassifiedLine::Blank);
            continue;
        }

        if trimmed == "QUERY PLAN" {
            results.push(ClassifiedLine::Header);
            continue;
        }

        if trimmed.starts_with("---") {
            results.push(ClassifiedLine::Separator);
            continue;
        }

        if let Some(sl) = try_parse_summary(trimmed, line_num) {
            results.push(ClassifiedLine::Summary(sl));
            continue;
        }

        let indent = working_line.chars().take_while(|c| *c == ' ').count();

        if is_node_line(trimmed) {
            if let Some(nl) = parse_node_line(trimmed, indent, line_num) {
                results.push(ClassifiedLine::Node(nl));
                continue;
            }
        }

        if let Some(pl) = try_parse_property(trimmed, indent, line_num) {
            results.push(ClassifiedLine::Property(pl));
            continue;
        }

        results.push(ClassifiedLine::Property(PropertyLine {
            indent,
            label: "Raw".to_string(),
            value: trimmed.to_string(),
            line_number: line_num,
        }));
    }

    results
}

fn is_node_line(trimmed: &str) -> bool {
    if RE_COST_INFO.is_match(trimmed) || RE_COST_WILDCARD.is_match(trimmed) {
        return true;
    }
    is_known_node_name(trimmed)
}

fn is_known_node_name(trimmed: &str) -> bool {
    if trimmed.contains(':') && !trimmed.contains("(cost=") && !trimmed.contains("(actual") {
        let before_colon = trimmed.split(':').next().unwrap_or("");
        if before_colon
            .chars()
            .all(|c| c.is_alphanumeric() || c == ' ' || c == '_' || c == '-' || c == '/')
        {
            return false;
        }
    }
    let text = if let Some(rest) = trimmed.strip_prefix("->") {
        rest.trim_start()
    } else {
        trimmed
    };
    if text.is_empty() {
        return false;
    }
    let name = extract_node_name(text);
    !name.is_empty() && is_recognized_node_prefix(&name)
}

fn extract_node_name(text: &str) -> String {
    let end = text.find(" on ").unwrap_or_else(|| {
        text.find('(')
            .unwrap_or_else(|| text.find("  ").unwrap_or(text.len()))
    });
    text[..end].trim().to_string()
}

fn is_recognized_node_prefix(name: &str) -> bool {
    let known_prefixes = [
        "Seq Scan",
        "Partitioned Seq Scan",
        "Sample Scan",
        "Index Scan",
        "Index Only Scan",
        "Partitioned Index",
        "Partitioned Bitmap",
        "Bitmap Index",
        "Bitmap Heap",
        "Tid Scan",
        "Tid Range Scan",
        "Subquery Scan",
        "Function Scan",
        "Values Scan",
        "CTE Scan",
        "WorkTable Scan",
        "Foreign Scan",
        "Partitioned Foreign Scan",
        "CStore Scan",
        "CStore Index",
        "ImCStore Scan",
        "TsStore Scan",
        "ANN Index Scan",
        "BitmapAnd",
        "BitmapOr",
        "Partition Iterator",
        "Vector Partition Iterator",
        "StartWith Op",
        "Replace",
        "Vector Insert",
        "Vector Update",
        "Vector Delete",
        "Vector Merge",
        "CStore Index And",
        "CStore Index Or",
        "Hash SetOp",
        "Vector Hash SetOp",
        "Partitioned Index Only Scan",
        "Partitioned Bitmap Heap Scan",
        "Partitioned Bitmap Index Scan",
        "Partitioned Tid Scan",
        "Vector Subquery Scan",
        "Vector Foreign Scan",
        "Nested Loop",
        "Hash Join",
        "Merge Join",
        "Vector Nest Loop",
        "Vector Hash Join",
        "Vector Sonic Hash Join",
        "Vector Merge Join",
        "Vector Asof Join",
        "Aggregate",
        "Group Aggregate",
        "Hash Aggregate",
        "HashAggregate",
        "Dummy HashAggregate",
        "Partial Hash Aggregate",
        "Vector Aggregate",
        "Vector Hash Aggregate",
        "Vector Sonic Hash Aggregate",
        "Vector Sort Aggregate",
        "Sort",
        "Group Sort",
        "Vector Sort",
        "Group",
        "Vector Group",
        "WindowAgg",
        "Vector WindowAgg",
        "Unique",
        "Vector Unique",
        "SetOp",
        "HashSetOp",
        "Vector SetOp",
        "Vector HashSetOp",
        "Append",
        "Vector Append",
        "Merge Append",
        "Vector Merge Append",
        "Limit",
        "Vector Limit",
        "Hash",
        "Materialize",
        "Result",
        "Vector Result",
        "Vec Result",
        "LockRows",
        "ModifyTable",
        "Insert",
        "Update",
        "Delete",
        "Row Adapter",
        "Vector Adapter",
        "Streaming",
        "Vector Streaming",
        "Data Node Scan",
        "Remote Subplan Scan",
        "Remote Query",
        "Partitioned CStore",
        "Row Adapter",
        "Vector Adapter",
        "Vector Materialize",
        "Gather",
        "Gather Merge",
        "ProjectSet",
        "Recursive Union",
        "WorkTable Scan",
        "Merge",
        "Merge Into",
        "Group",
        "Vector Group",
    ];
    known_prefixes
        .iter()
        .any(|p| name == *p || name.starts_with(p))
}

fn parse_node_line(trimmed: &str, indent: usize, line_number: usize) -> Option<NodeLine> {
    let is_child = trimmed.starts_with("->");

    let working = if is_child {
        trimmed.strip_prefix("->")?.trim_start()
    } else {
        trimmed
    };

    let cost = RE_COST_INFO.captures(trimmed).map(|caps| {
        let pred_time = RE_PRED_INFO
            .captures(trimmed)
            .and_then(|c| c.get(1))
            .map(|m| m.as_str().parse().unwrap_or(0.0));
        let pred_rows = RE_PRED_INFO
            .captures(trimmed)
            .and_then(|c| c.get(2))
            .map(|m| m.as_str().parse().unwrap_or(0.0));

        CostInfo {
            startup_cost: caps[1].parse().unwrap_or(0.0),
            total_cost: caps[2].parse().unwrap_or(0.0),
            plan_rows: caps[3].parse().unwrap_or(0.0),
            plan_width: caps[4].parse().unwrap_or(0),
            pred_time,
            pred_rows,
        }
    });

    let actual = if RE_NEVER_EXECUTED.is_match(trimmed) || RE_UNKNOWN_ACTUAL.is_match(trimmed) {
        Some(ActualInfo {
            startup_time_ms: None,
            total_time_ms: None,
            rows: 0.0,
            loops: 1.0,
            executed: false,
        })
    } else if let Some(caps) = RE_ACTUAL_TIME.captures(trimmed) {
        Some(ActualInfo {
            startup_time_ms: Some(caps[1].parse().unwrap_or(0.0)),
            total_time_ms: Some(caps[2].parse().unwrap_or(0.0)),
            rows: caps[3].parse().unwrap_or(0.0),
            loops: caps[4].parse().unwrap_or(0.0),
            executed: true,
        })
    } else {
        RE_ACTUAL_ROWS.captures(trimmed).map(|caps| ActualInfo {
            startup_time_ms: None,
            total_time_ms: None,
            rows: caps[1].parse().unwrap_or(0.0),
            loops: caps[2].parse().unwrap_or(0.0),
            executed: true,
        })
    };

    let node_text_end = RE_COST_INFO
        .find(working)
        .map(|m| m.start())
        .or_else(|| RE_COST_WILDCARD.find(working).map(|m| m.start()))
        .unwrap_or(working.len());
    let raw_node_text = working[..node_text_end].trim().to_string();

    let indent_level = if is_child { (indent / 2).max(1) } else { 0 };

    Some(NodeLine {
        indent,
        is_child,
        raw_node_text,
        cost,
        actual,
        line_number,
        indent_level,
    })
}

fn try_parse_property(trimmed: &str, indent: usize, line_number: usize) -> Option<PropertyLine> {
    let colon_pos = trimmed.find(':')?;

    if colon_pos == 0 {
        return None;
    }

    let label = &trimmed[..colon_pos];

    if !label
        .chars()
        .all(|c| c.is_alphanumeric() || c == ' ' || c == '_' || c == '-' || c == '/' || c == '.')
    {
        return None;
    }

    let value = trimmed[colon_pos + 1..].trim().to_string();

    Some(PropertyLine {
        indent,
        label: label.trim().to_string(),
        value,
        line_number,
    })
}

fn try_parse_summary(trimmed: &str, line_number: usize) -> Option<SummaryLine> {
    let summary_prefixes = [
        "Total runtime:",
        "Total runtime ",
        "Peak Memory:",
        "Peak Memory ",
        "Planner runtime:",
        "Planner runtime ",
        "Plan size:",
        "Plan size ",
        "Query Id:",
        "Query Id ",
        "Executor Start:",
        "Executor Start ",
        "Executor Run:",
        "Executor Run ",
        "Executor End:",
        "Executor End ",
        "Total Network:",
        "Total Network ",
    ];

    let is_summary = summary_prefixes.iter().any(|p| trimmed.starts_with(p));
    if !is_summary {
        return None;
    }

    let colon_pos = trimmed.find(':')?;
    let label = trimmed[..colon_pos].trim().to_string();
    let value = trimmed[colon_pos + 1..].trim().to_string();

    Some(SummaryLine {
        entries: vec![(label, value)],
        line_number,
    })
}
