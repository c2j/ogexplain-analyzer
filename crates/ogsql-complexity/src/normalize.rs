use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

/// Normalize SQL by replacing literals with `?` placeholders.
///
/// Replaces:
/// - String literals: `'foo'` → `?`
/// - Numeric literals: `42`, `3.14` → `?`
/// - IN list values: `IN (1, 2, 3)` → `IN (?)`
///
/// Preserves keywords, identifiers, operators, and case.
pub fn normalize_sql(sql: &str) -> String {
    let result = replace_string_literals(sql);
    let result = replace_numeric_literals(&result);
    let result = collapse_in_lists(&result);
    let result = normalize_whitespace(&result);
    result.trim().to_string()
}

/// Compute a short template ID from SQL.
///
/// Returns `tpl_` prefix + 12 hex chars of the normalized SQL hash.
/// Stable for same input; groups similar SQLs under the same ID.
const TEMPLATE_ID_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;

pub fn template_id(sql: &str) -> String {
    let normalized = normalize_sql(sql);
    let mut hasher = DefaultHasher::new();
    normalized.hash(&mut hasher);
    format!("tpl_{:012x}", hasher.finish() & TEMPLATE_ID_MASK)
}

fn replace_string_literals(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '\'' {
            result.push('?');
            while let Some(&next) = chars.peek() {
                if next == '\'' {
                    chars.next();
                    if chars.peek() == Some(&'\'') {
                        chars.next();
                        continue;
                    }
                    break;
                }
                chars.next();
            }
        } else {
            result.push(c);
        }
    }
    result
}

fn replace_numeric_literals(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let len = chars.len();
    let mut result = String::with_capacity(s.len());
    let mut i = 0;

    while i < len {
        let c = chars[i];

        let is_number_start =
            c.is_ascii_digit() || (c == '-' && i + 1 < len && chars[i + 1].is_ascii_digit());

        if is_number_start {
            let is_part_of_identifier =
                i > 0 && (chars[i - 1].is_ascii_alphanumeric() || chars[i - 1] == '_');

            if !is_part_of_identifier {
                let start = i;
                if c == '-' {
                    i += 1;
                }
                while i < len && chars[i].is_ascii_digit() {
                    i += 1;
                }
                if i < len && chars[i] == '.' && i + 1 < len && chars[i + 1].is_ascii_digit() {
                    i += 1;
                    while i < len && chars[i].is_ascii_digit() {
                        i += 1;
                    }
                }
                if i < len && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                    result.push_str(&s[start..i]);
                } else {
                    result.push('?');
                }
                continue;
            }
        }

        result.push(c);
        i += 1;
    }

    result
}

fn collapse_in_lists(s: &str) -> String {
    let re = regex::Regex::new(r"(?i)\bIN\s*\(\s*(\?\s*(,\s*\?\s*)*)\s*\)").unwrap();
    re.replace_all(s, "IN (?)").to_string()
}

fn normalize_whitespace(s: &str) -> String {
    let re = regex::Regex::new(r"\s+").unwrap();
    re.replace_all(s.trim(), " ").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_replaces_string_literals() {
        assert_eq!(normalize_sql("WHERE name = 'foo'"), "WHERE name = ?");
    }

    #[test]
    fn test_normalize_replaces_numeric_literals() {
        assert_eq!(normalize_sql("WHERE id = 42"), "WHERE id = ?");
        assert_eq!(normalize_sql("WHERE price = 3.14"), "WHERE price = ?");
    }

    #[test]
    fn test_normalize_collapses_in_lists() {
        let input = "WHERE id IN (1, 2, 3, 4, 5)";
        let result = normalize_sql(input);
        assert_eq!(result, "WHERE id IN (?)");
    }

    #[test]
    fn test_normalize_preserves_keywords() {
        assert_eq!(normalize_sql("SELECT * FROM t"), "SELECT * FROM t");
    }

    #[test]
    fn test_normalize_preserves_case() {
        let result = normalize_sql("SELECT ColName FROM MyTable");
        assert!(result.contains("ColName"));
        assert!(result.contains("MyTable"));
    }

    #[test]
    fn test_template_id_stable() {
        let id1 = template_id("SELECT * FROM users WHERE id = 1");
        let id2 = template_id("SELECT * FROM users WHERE id = 1");
        assert_eq!(id1, id2);
    }

    #[test]
    fn test_template_id_groups_similar() {
        let id1 = template_id("SELECT name FROM emp WHERE id = '123'");
        let id2 = template_id("SELECT name FROM emp WHERE id = '456'");
        assert_eq!(id1, id2);
    }

    #[test]
    fn test_template_id_different_queries() {
        let id1 = template_id("SELECT * FROM a");
        let id2 = template_id("SELECT * FROM b");
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_template_id_format() {
        let id = template_id("SELECT 1");
        assert!(id.starts_with("tpl_"));
        assert_eq!(id.len(), 16);
    }

    #[test]
    fn test_normalize_handles_escaped_quotes() {
        let result = normalize_sql("WHERE name = 'O''Brien'");
        assert_eq!(result, "WHERE name = ?");
    }

    #[test]
    fn test_normalize_empty_string() {
        assert_eq!(normalize_sql(""), "");
    }

    #[test]
    fn test_normalize_multiple_string_literals() {
        let result = normalize_sql("SELECT * FROM t WHERE a = 'hello' AND b = 'world'");
        assert_eq!(result, "SELECT * FROM t WHERE a = ? AND b = ?");
    }

    #[test]
    fn test_normalize_in_with_strings() {
        let input = "WHERE status IN ('active', 'pending', 'done')";
        assert_eq!(normalize_sql(input), "WHERE status IN (?)");
    }

    #[test]
    fn test_normalize_negative_numbers() {
        assert_eq!(normalize_sql("WHERE temp = -5"), "WHERE temp = ?");
    }

    #[test]
    fn test_normalize_preserves_identifiers_with_numbers() {
        let result = normalize_sql("SELECT t1.col_123 FROM t1 WHERE t1.id = 42");
        assert!(result.contains("t1.col_123"));
        assert!(result.contains("t1.id"));
        assert!(result.ends_with("= ?"));
    }
}
