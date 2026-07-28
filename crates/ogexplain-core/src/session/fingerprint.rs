use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use crate::model::ExplainPlan;

/// Produce a stable hash for a plan's structural shape.
///
/// Two plans with the same node types, relation, and filter/sort/index
/// conditions produce the same fingerprint — even if runtimes differ.
/// This is used to group repeated queries (e.g. inside a loop).
///
/// Note: uses `std::collections::hash_map::DefaultHasher`, which is NOT
/// guaranteed stable across Rust stdlib versions. Fingerprints should not
/// be persisted across builds.
pub(crate) fn plan_fingerprint(plan: &ExplainPlan) -> u64 {
    let mut hasher = DefaultHasher::new();
    hash_node(&plan.root, &mut hasher);
    hasher.finish()
}

fn hash_node(node: &crate::model::PlanNode, hasher: &mut DefaultHasher) {
    node.node_type.to_string().hash(hasher);
    node.relation.hash(hasher);

    for prop in &node.properties {
        if matches!(
            prop.label.as_str(),
            "Filter"
                | "Sort Key"
                | "Merge Sort Key"
                | "Index Cond"
                | "Hash Cond"
                | "Join Filter"
                | "Merge Cond"
                | "Group By Key"
                | "Distribute Key"
        ) {
            prop.label.hash(hasher);
            prop.value.hash(hasher);
        }
    }

    for child in &node.children {
        hash_node(child, hasher);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse;

    #[test]
    fn same_plan_same_fingerprint() {
        let input_a = "Seq Scan on t  (cost=0.00..10.00 rows=5 width=36)";
        let input_b = "Seq Scan on t  (cost=0.00..12.00 rows=8 width=36)";
        let a = parse(input_a).unwrap();
        let b = parse(input_b).unwrap();
        assert_eq!(plan_fingerprint(&a), plan_fingerprint(&b));
    }

    #[test]
    fn different_relation_different_fingerprint() {
        let a = parse("Seq Scan on orders  (cost=0.00..10.00 rows=5 width=36)").unwrap();
        let b = parse("Seq Scan on users  (cost=0.00..10.00 rows=5 width=36)").unwrap();
        assert_ne!(plan_fingerprint(&a), plan_fingerprint(&b));
    }

    #[test]
    fn different_node_type_different_fingerprint() {
        let a = parse("Seq Scan on t  (cost=0.00..10.00 rows=5 width=36)").unwrap();
        let b = parse("Sort  (cost=10.00..10.05 rows=5 width=36)\n  Sort Key: x\n  ->  Seq Scan on t  (cost=0.00..10.00 rows=5 width=36)").unwrap();
        assert_ne!(plan_fingerprint(&a), plan_fingerprint(&b));
    }

    #[test]
    fn different_filter_different_fingerprint() {
        let a =
            parse("Seq Scan on t  (cost=0.00..10.00 rows=5 width=36)\n  Filter: (a = 1)").unwrap();
        let b =
            parse("Seq Scan on t  (cost=0.00..10.00 rows=5 width=36)\n  Filter: (b = 2)").unwrap();
        assert_ne!(plan_fingerprint(&a), plan_fingerprint(&b));
    }

    #[test]
    fn function_scan_vs_seq_scan_different() {
        let a = parse("Function Scan on fn_x  (cost=0.25..0.26 rows=1 width=4)").unwrap();
        let b = parse("Seq Scan on fn_x  (cost=0.00..10.00 rows=5 width=36)").unwrap();
        assert_ne!(plan_fingerprint(&a), plan_fingerprint(&b));
    }

    #[test]
    fn nested_plan_fingerprint_stable() {
        let input = "Sort  (cost=5.50..5.51 rows=1 width=100)\n  Sort Key: salary DESC\n  ->  Seq Scan on employees  (cost=0.00..5.49 rows=1 width=100)\n        Filter: (dept_id = 1)";
        let a = parse(input).unwrap();
        let b = parse(input).unwrap();
        assert_eq!(plan_fingerprint(&a), plan_fingerprint(&b));
    }
}
