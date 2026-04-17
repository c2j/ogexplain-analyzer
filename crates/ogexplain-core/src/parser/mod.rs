mod line_classifier;
mod tree_builder;

use crate::model::ExplainPlan;

#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error("Failed to parse line {line}: {message}")]
    LineParse { line: usize, message: String },
    #[error("Empty input")]
    EmptyInput,
    #[error("No plan nodes found")]
    NoPlanNodes,
}

pub fn parse(text: &str) -> Result<ExplainPlan, ParseError> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err(ParseError::EmptyInput);
    }

    if looks_like_mixed_content(trimmed) {
        let blocks = extract_explain_blocks(text);
        if let Some(block) = blocks.first() {
            let block_classified = line_classifier::classify_lines(block);
            if let Ok(plan) = tree_builder::build_tree(&block_classified) {
                return Ok(plan);
            }
        }
    }

    let classified = line_classifier::classify_lines(trimmed);
    if let Ok(plan) = tree_builder::build_tree(&classified) {
        return Ok(plan);
    }

    let blocks = extract_explain_blocks(text);
    if let Some(block) = blocks.first() {
        let block_classified = line_classifier::classify_lines(block);
        return tree_builder::build_tree(&block_classified);
    }

    Err(ParseError::NoPlanNodes)
}

pub fn parse_multi(text: &str) -> Result<Vec<ExplainPlan>, ParseError> {
    let blocks = extract_explain_blocks(text);
    if blocks.is_empty() {
        return Err(ParseError::NoPlanNodes);
    }
    blocks.iter().map(|block| parse(block)).collect()
}

fn extract_explain_blocks(text: &str) -> Vec<String> {
    let mut blocks: Vec<String> = Vec::new();
    let mut current_block: Vec<String> = Vec::new();
    let mut in_block = false;

    for raw_line in text.lines() {
        let line = strip_regression_marker(raw_line);
        let trimmed = line.trim();

        if is_sql_line(trimmed) || is_rows_footer(trimmed) || is_server_message(trimmed) {
            if in_block && !current_block.is_empty() {
                let block_text = current_block.join("\n");
                if has_node_content(&block_text) {
                    blocks.push(block_text);
                }
                current_block.clear();
            }
            in_block = false;
            continue;
        }

        if trimmed.is_empty() {
            if in_block {
                current_block.push(String::new());
            }
            continue;
        }

        if trimmed == "QUERY PLAN" || trimmed.starts_with("---") {
            in_block = true;
            continue;
        }

        if is_explain_content(trimmed) {
            in_block = true;
            current_block.push(line.to_string());
        } else if in_block {
            current_block.push(line.to_string());
        }
    }

    if !current_block.is_empty() {
        let block_text = current_block.join("\n");
        if has_node_content(&block_text) {
            blocks.push(block_text);
        }
    }

    blocks
}

fn strip_regression_marker(line: &str) -> &str {
    line.strip_prefix("--?").unwrap_or(line)
}

pub(crate) fn is_sql_line(s: &str) -> bool {
    let lower = s.to_lowercase();
    lower.starts_with("create ")
        || lower.starts_with("insert ")
        || lower.starts_with("drop ")
        || lower.starts_with("set ")
        || lower.starts_with("prepare ")
        || lower.starts_with("deallocate ")
        || is_explain_sql_command(&lower)
        || lower.starts_with("/*")
        || is_sql_comment(&lower)
        || lower.starts_with("analyze ")
        || lower.starts_with("alter ")
}

fn is_sql_comment(lower: &str) -> bool {
    lower.starts_with("-- ") || lower == "--"
}

pub(crate) fn is_explain_sql_command(lower: &str) -> bool {
    if !lower.starts_with("explain ") {
        return false;
    }
    let rest = &lower["explain ".len()..];
    rest.starts_with("select ")
        || rest.starts_with("verbose ")
        || rest.starts_with("performance ")
        || rest.starts_with("insert ")
        || rest.starts_with("execute ")
        || rest.starts_with("analyze ")
        || rest.starts_with("(")
        || rest.starts_with("update ")
        || rest.starts_with("delete ")
}

fn is_rows_footer(s: &str) -> bool {
    s.starts_with('(') && s.ends_with(" rows)") || s == "(1 row)"
}

fn is_server_message(s: &str) -> bool {
    s.starts_with("NOTICE:")
        || s.starts_with("DETAIL:")
        || s.starts_with("HINT:")
        || s.starts_with("ERROR:")
        || s.starts_with("CONTEXT:")
        || s.starts_with("WARNING:")
}

fn is_explain_content(s: &str) -> bool {
    if s.contains("(cost=") || s.contains("(actual time=") {
        return true;
    }
    let property_prefixes = [
        "Output:",
        "Filter:",
        "Sort Key:",
        "Merge Sort Key:",
        "Group By Key:",
        "Hash Cond:",
        "Join Filter:",
        "Merge Cond:",
        "Index Cond:",
        "Recheck Cond:",
        "Sort Method:",
        "Hash Buckets:",
        "Hash Batches:",
        "Original Hash Buckets:",
        "Original Hash Batches:",
        "Peak Memory:",
        "Sort Space Used:",
        "Sort Space Type:",
        "Bloom Filter:",
        "DFS file pruning:",
        "InitPlan",
        "SubPlan",
        "Rows Removed",
        "Node/s:",
        "Node group:",
        "(CPU:",
        "(Buffers:",
        "Memory Usage:",
        "Distribute Key:",
        "Spawn on:",
        "Consumer Nodes:",
        "Remote SQL:",
        "Remote plan:",
    ];
    if property_prefixes.iter().any(|p| s.starts_with(p)) {
        return true;
    }
    let text = if let Some(rest) = s.strip_prefix("->") {
        rest.trim_start()
    } else {
        s
    };
    is_plan_node_name(text)
}

fn is_plan_node_name(text: &str) -> bool {
    if text.is_empty() {
        return false;
    }
    let end = text.find('(').unwrap_or_else(|| {
        text.find(" on ")
            .unwrap_or_else(|| text.find("  ").unwrap_or(text.len()))
    });
    let mut name = text[..end].trim();
    if let Some(pos) = name.find(" using ") {
        name = &name[..pos];
    }
    let known_names = [
        "Aggregate",
        "Group Aggregate",
        "Hash Aggregate",
        "HashAggregate",
        "Dummy HashAggregate",
        "Partial Hash Aggregate",
        "Append",
        "Merge Append",
        "Bitmap Heap Scan",
        "Bitmap Index Scan",
        "CTE Scan",
        "WorkTable Scan",
        "CStore Scan",
        "CStore Index Scan",
        "CStore Index Ctid Scan",
        "CStore Index Heap Scan",
        "Data Node Scan",
        "Delete",
        "Foreign Scan",
        "Function Scan",
        "Gather",
        "Gather Merge",
        "Group",
        "Group Sort",
        "Hash",
        "Hash Join",
        "HashSetOp",
        "Hash SetOp",
        "ImCStore Scan",
        "Index Scan",
        "Index Only Scan",
        "Insert",
        "Limit",
        "LockRows",
        "Materialize",
        "Merge",
        "Merge Join",
        "Merge Into",
        "ModifyTable",
        "Nested Loop",
        "Partitioned Seq Scan",
        "Partitioned Index Scan",
        "ProjectSet",
        "Recursive Union",
        "Result",
        "Row Adapter",
        "Remote Query",
        "Remote Subplan Scan",
        "Sample Scan",
        "Seq Scan",
        "SetOp",
        "Sort",
        "Subquery Scan",
        "Streaming",
        "Tid Scan",
        "Tid Range Scan",
        "TsStore Scan",
        "Unique",
        "Update",
        "Values Scan",
        "Vector Adapter",
        "Vector Aggregate",
        "Vector Append",
        "Vector Foreign Scan",
        "Vector Group",
        "Vector Hash Join",
        "Vector Hash Aggregate",
        "Vector HashSetOp",
        "Vector Hash SetOp",
        "Vector Limit",
        "Vector Materialize",
        "Vector Merge Append",
        "Vector Merge Join",
        "Vector Nest Loop",
        "Vector Result",
        "Vector SetOp",
        "Vector Sonic Hash Aggregate",
        "Vector Sonic Hash Join",
        "Vector Sort",
        "Vector Sort Aggregate",
        "Vector Streaming",
        "Vector Subquery Scan",
        "Vector Unique",
        "Vector WindowAgg",
        "WindowAgg",
        "ANN Index Scan",
        "Vector Asof Join",
        "BitmapAnd",
        "BitmapOr",
        "Partitioned Bitmap Heap Scan",
        "Partitioned Bitmap Index Scan",
        "Partitioned Index Only Scan",
        "Partitioned CStore Scan",
        "Partitioned Foreign Scan",
        "Partitioned Tid Scan",
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
    ];
    known_names.contains(&name)
}

fn has_node_content(block: &str) -> bool {
    if block.contains("(cost=") {
        return true;
    }
    for line in block.lines() {
        let text = if let Some(rest) = line.trim().strip_prefix("->") {
            rest.trim_start()
        } else {
            line.trim()
        };
        if is_plan_node_name(text) {
            return true;
        }
    }
    false
}

fn looks_like_mixed_content(text: &str) -> bool {
    let mut has_sql = false;
    let mut has_rows_footer = false;
    for line in text.lines() {
        let cleaned = strip_regression_marker(line);
        let trimmed = cleaned.trim();
        if is_sql_line(trimmed) {
            has_sql = true;
        }
        if is_rows_footer(trimmed) {
            has_rows_footer = true;
        }
    }
    has_sql || has_rows_footer
}
