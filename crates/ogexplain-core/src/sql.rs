use serde::Serialize;

/// Extracted content from mixed SQL + EXPLAIN input.
#[derive(Debug, Clone, Default, Serialize)]
pub struct ExtractedContent {
    pub sql_lines: Vec<String>,
    pub sql_text: String,
    pub has_sql: bool,
}

/// A paired segment: SQL text (if any) followed by its EXPLAIN block.
#[derive(Debug, Clone)]
pub struct InputBlock {
    pub sql_text: Option<String>,
    pub explain_text: String,
}

/// Split input into ordered SQL+EXPLAIN pairs.
///
/// Handles three patterns:
/// - `SQL; QUERY PLAN ... (N rows)` — paired
/// - `EXPLAIN SELECT ...; QUERY PLAN ...` — paired (SQL extracted from EXPLAIN line)
/// - Pure EXPLAIN (no SQL) — block with `sql_text: None`
pub fn segment_input(text: &str) -> Vec<InputBlock> {
    let mut blocks: Vec<InputBlock> = Vec::new();
    let mut current_sql: Vec<String> = Vec::new();
    let mut current_explain: Vec<String> = Vec::new();
    let mut in_sql = false;
    let mut in_explain = false;
    let mut prev_sql_ended_with_semi = false;

    let finalize_explain = |explain_lines: &[String]| -> Option<String> {
        let text = explain_lines.join("\n");
        let trimmed = text.trim();
        if trimmed.is_empty()
            || !trimmed.contains("(cost=")
                && !trimmed.contains("->")
                && !looks_like_plan_node(trimmed)
        {
            return None;
        }
        Some(text)
    };

    for raw_line in text.lines() {
        let line = raw_line.strip_prefix("--?").unwrap_or(raw_line);
        let trimmed = line.trim();

        if trimmed.is_empty() {
            continue;
        }

        if is_separator_comment(trimmed) && !in_explain {
            if !current_explain.is_empty() {
                let sql = drain_sql(&mut current_sql, &mut in_sql);
                let explain = finalize_explain(&current_explain);
                if let Some(explain) = explain {
                    blocks.push(InputBlock {
                        sql_text: sql,
                        explain_text: explain,
                    });
                }
            } else if !current_sql.is_empty() {
                let sql = drain_sql(&mut current_sql, &mut in_sql);
                if sql.is_some() {
                    blocks.push(InputBlock {
                        sql_text: sql,
                        explain_text: String::new(),
                    });
                }
            }
            current_explain.clear();
            in_explain = false;
            prev_sql_ended_with_semi = false;
            continue;
        }

        let is_sql =
            !in_explain && (is_sql_statement_start(trimmed) || crate::parser::is_sql_line(trimmed));
        let is_explain_marker = trimmed == "QUERY PLAN" || trimmed.starts_with("---");
        let is_explain_line = is_explain_output(trimmed) || is_explain_marker;
        let is_rows_footer =
            trimmed.starts_with('(') && trimmed.ends_with(" rows)") || trimmed == "(1 row)";

        if is_rows_footer {
            if in_explain && !current_explain.is_empty() {
                let sql = drain_sql(&mut current_sql, &mut in_sql);
                let explain = finalize_explain(&current_explain);
                if let Some(explain) = explain {
                    blocks.push(InputBlock {
                        sql_text: sql,
                        explain_text: explain,
                    });
                }
            } else if in_explain || in_sql {
                let sql = drain_sql(&mut current_sql, &mut in_sql);
                if sql.is_some() {
                    blocks.push(InputBlock {
                        sql_text: sql,
                        explain_text: String::new(),
                    });
                }
            }
            current_explain.clear();
            in_explain = false;
            prev_sql_ended_with_semi = false;
            continue;
        }

        if is_explain_line {
            in_explain = true;
            prev_sql_ended_with_semi = false;
            if !is_explain_marker {
                current_explain.push(line.to_string());
            }
            continue;
        }

        if is_sql && !in_explain {
            if prev_sql_ended_with_semi && !current_sql.is_empty() {
                let prev_sql = drain_sql(&mut current_sql, &mut in_sql);
                if prev_sql.is_some() {
                    blocks.push(InputBlock {
                        sql_text: prev_sql,
                        explain_text: String::new(),
                    });
                }
            }
            in_sql = true;
            let sql_part = if crate::parser::is_explain_sql_command(&trimmed.to_lowercase()) {
                extract_sql_from_explain_line(trimmed)
            } else {
                Some(trimmed.to_string())
            };
            if let Some(part) = sql_part {
                current_sql.push(part);
            }
            prev_sql_ended_with_semi = trimmed.ends_with(';');
            continue;
        }

        if in_explain {
            current_explain.push(line.to_string());
        } else if in_sql {
            let sql_part = if crate::parser::is_explain_sql_command(&trimmed.to_lowercase()) {
                extract_sql_from_explain_line(trimmed)
            } else {
                Some(trimmed.to_string())
            };
            if let Some(part) = sql_part {
                current_sql.push(part);
            }
            prev_sql_ended_with_semi = trimmed.ends_with(';');
        }
    }

    if !current_explain.is_empty() {
        let sql = drain_sql(&mut current_sql, &mut in_sql);
        let explain = finalize_explain(&current_explain);
        if let Some(explain) = explain {
            blocks.push(InputBlock {
                sql_text: sql,
                explain_text: explain,
            });
        }
    } else if !current_sql.is_empty() {
        let sql = drain_sql(&mut current_sql, &mut in_sql);
        blocks.push(InputBlock {
            sql_text: sql,
            explain_text: String::new(),
        });
    }

    blocks
}

fn drain_sql(current_sql: &mut Vec<String>, in_sql: &mut bool) -> Option<String> {
    *in_sql = false;
    if current_sql.is_empty() {
        return None;
    }
    let text: String = current_sql
        .drain(..)
        .filter(|l| !l.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    if text.trim().is_empty() {
        None
    } else {
        Some(text)
    }
}

fn looks_like_plan_node(s: &str) -> bool {
    let lower = s.to_lowercase();
    lower.contains("seq scan")
        || lower.contains("index scan")
        || lower.contains("hash join")
        || lower.contains("merge join")
        || lower.contains("nested loop")
        || lower.contains("sort")
        || lower.contains("limit")
        || lower.contains("aggregate")
        || lower.contains("hash")
        || lower.contains("gather")
        || lower.contains("streaming")
}

impl ExtractedContent {
    pub fn from_text(text: &str) -> Self {
        let mut sql_lines: Vec<String> = Vec::new();
        let mut current_sql_block: Vec<String> = Vec::new();
        let mut in_sql_block = false;

        for raw_line in text.lines() {
            let line = raw_line.strip_prefix("--?").unwrap_or(raw_line);
            let trimmed = line.trim();

            if trimmed.is_empty() {
                if !current_sql_block.is_empty() {
                    current_sql_block.push(String::new());
                }
                continue;
            }

            if is_explain_output(trimmed) {
                if !current_sql_block.is_empty() {
                    let block: Vec<String> = current_sql_block
                        .drain(..)
                        .filter(|l| !l.trim().is_empty())
                        .collect();
                    sql_lines.extend(block);
                    in_sql_block = false;
                }
                continue;
            }

            if in_sql_block {
                let sql_part = if crate::parser::is_explain_sql_command(&trimmed.to_lowercase()) {
                    extract_sql_from_explain_line(trimmed)
                } else {
                    Some(trimmed.to_string())
                };
                if let Some(part) = sql_part {
                    current_sql_block.push(part);
                }
                continue;
            }

            if is_sql_statement_start(trimmed) || crate::parser::is_sql_line(trimmed) {
                in_sql_block = true;
                let sql_part = if crate::parser::is_explain_sql_command(&trimmed.to_lowercase()) {
                    extract_sql_from_explain_line(trimmed)
                } else {
                    Some(trimmed.to_string())
                };
                if let Some(part) = sql_part {
                    current_sql_block.push(part);
                }
            }
        }

        if !current_sql_block.is_empty() {
            let block: Vec<String> = current_sql_block
                .drain(..)
                .filter(|l| !l.trim().is_empty())
                .collect();
            sql_lines.extend(block);
        }

        let sql_text = sql_lines.join("\n");
        let has_sql = !sql_text.trim().is_empty();

        Self {
            sql_lines,
            sql_text,
            has_sql,
        }
    }
}

/// Check if a line is a separator comment like `-- ===`, `-- ###`, `-- ---`, `-- @@@`, `-- ***`.
fn is_separator_comment(s: &str) -> bool {
    if !s.starts_with("--") {
        return false;
    }
    let rest = s[2..].trim();
    if rest.is_empty() {
        return false;
    }
    let first = rest.chars().next().unwrap();
    if !matches!(first, '=' | '@' | '#' | '-' | '*') {
        return false;
    }
    rest.len() >= 3 && rest.chars().all(|c| c == first)
}

fn is_sql_statement_start(s: &str) -> bool {
    let lower = s.to_lowercase();
    lower.starts_with("select ")
        || lower.starts_with("with ")
        || lower.starts_with("update ")
        || lower.starts_with("delete ")
        || lower == "select"
        || lower == "with"
        || lower == "update"
        || lower == "delete"
}

/// Check if a line is part of EXPLAIN output (not SQL).
fn is_explain_output(s: &str) -> bool {
    s == "QUERY PLAN"
        || s.starts_with("---")
        || s.contains("(cost=")
        || s.contains("(actual time=")
        || s.starts_with("Output:")
        || s.starts_with("Filter:")
        || s.starts_with("Sort Key:")
        || s.starts_with("Hash Cond:")
        || s.starts_with("Join Filter:")
        || s.starts_with("Index Cond:")
        || s.starts_with("Group By Key:")
        || s.starts_with("->")
        || (s.starts_with('(') && s.ends_with(" rows)"))
        || s == "(1 row)"
        || s.starts_with("NOTICE:")
        || s.starts_with("DETAIL:")
        || s.starts_with("HINT:")
        || s.starts_with("ERROR:")
        || s.starts_with("CONTEXT:")
        || s.starts_with("WARNING:")
}

/// If line is `EXPLAIN [ANALYZE] [VERBOSE] SELECT ...`, extract just the SQL part.
fn extract_sql_from_explain_line(s: &str) -> Option<String> {
    let lower = s.to_lowercase();
    if !lower.starts_with("explain ") {
        return None;
    }

    let rest = &s["explain ".len()..];
    let mut sql_rest = rest.trim_start();

    loop {
        let lower_rest = sql_rest.to_lowercase();
        if lower_rest.starts_with("analyze ") {
            sql_rest = &sql_rest["analyze ".len()..];
        } else if lower_rest.starts_with("verbose ") {
            sql_rest = &sql_rest["verbose ".len()..];
        } else if lower_rest.starts_with("performance ") {
            sql_rest = &sql_rest["performance ".len()..];
        } else {
            break;
        }
    }

    if sql_rest.trim().is_empty() {
        return None;
    }

    Some(sql_rest.to_string())
}
