use ogexplain_core::model::{NodeType, StreamingType};
use ogexplain_core::{parse, parse_multi};

#[test]
fn basic_seq_scan_parses() {
    let input = "QUERY PLAN\n----------------------------------------------------\nSeq Scan on t1  (cost=0.00..12.00 rows=100 width=4)";
    let plan = parse(input).unwrap();
    assert_eq!(plan.root.node_type, NodeType::SeqScan);
    assert_eq!(plan.root.relation, Some("t1".to_string()));
    assert!(plan.root.estimated.is_some());
    let est = plan.root.estimated.as_ref().unwrap();
    assert_eq!(est.startup_cost, 0.0);
    assert_eq!(est.total_cost, 12.0);
    assert_eq!(est.plan_rows, 100.0);
    assert_eq!(est.plan_width, 4);
    assert!(plan.root.actual.is_none());
    assert!(plan.root.children.is_empty());
}

#[test]
fn index_scan_with_using_clause() {
    let input = "QUERY PLAN\n----------------------------------------------------\nIndex Scan using idx_status on orders  (cost=0.29..8.31 rows=1 width=68) (actual time=0.034..0.036 rows=1 loops=1)\n  Index Cond: (status = 'shipped')\n  Filter: (total > 1000)\n  Rows Removed by Filter: 3";
    let plan = parse(input).unwrap();
    assert_eq!(plan.root.node_type, NodeType::IndexScan);
    assert_eq!(plan.root.relation, Some("orders".to_string()));
    assert!(plan.root.estimated.is_some());
    assert!(plan.root.actual.is_some());
    let actual = plan.root.actual.as_ref().unwrap();
    assert_eq!(actual.rows, 1.0);
    assert_eq!(actual.loops, 1.0);
    assert!(actual.executed);

    let labels: Vec<&str> = plan
        .root
        .properties
        .iter()
        .map(|p| p.label.as_str())
        .collect();
    assert!(labels.contains(&"Index Cond"));
    assert!(labels.contains(&"Filter"));
    assert!(labels.contains(&"Rows Removed by Filter"));

    let structured = plan.root.structured_props.as_ref().unwrap();
    assert_eq!(structured.rows_removed_by_filter, Some(3.0));
}

#[test]
fn hash_join_tree_structure() {
    let input = "QUERY PLAN\n----------------------------------------------------\nHash Join  (cost=25.38..63.85 rows=200 width=16) (actual time=0.082..0.198 rows=200 loops=1)\n   Hash Cond: (o.customer_id = c.id)\n   ->  Seq Scan on orders o  (cost=0.00..18.50 rows=850 width=12) (actual time=0.008..0.042 rows=850 loops=1)\n   ->  Hash  (cost=15.20..15.20 rows=520 width=8) (actual time=0.035..0.035 rows=520 loops=1)\n         ->  Seq Scan on customers c  (cost=0.00..15.20 rows=520 width=8) (actual time=0.005..0.020 rows=520 loops=1)";
    let plan = parse(input).unwrap();
    assert_eq!(plan.root.node_type, NodeType::HashJoin);
    assert_eq!(plan.root.children.len(), 2);

    let left = &plan.root.children[0];
    assert_eq!(left.node_type, NodeType::SeqScan);
    assert_eq!(left.relation, Some("orders".to_string()));

    let right = &plan.root.children[1];
    assert_eq!(right.node_type, NodeType::Hash);
    assert_eq!(right.children.len(), 1);
    assert_eq!(right.children[0].node_type, NodeType::SeqScan);
    assert_eq!(right.children[0].relation, Some("customers".to_string()));

    let hash_cond = plan
        .root
        .properties
        .iter()
        .find(|p| p.label == "Hash Cond")
        .expect("Hash Cond property missing");
    assert_eq!(hash_cond.value, "(o.customer_id = c.id)");
}

#[test]
fn pretty_mode_with_numeric_prefix() {
    let input = "QUERY PLAN\n-----------------------------------------------------------------------------------------------------------------------------\n1 --Hash Join  (cost=25.38..63.85 rows=200 width=16) (actual time=0.082..0.198 rows=200 loops=1)\n   Hash Cond: (o.customer_id = c.id)\n   ->  2 --Seq Scan on orders o  (cost=0.00..18.50 rows=850 width=12) (actual time=0.008..0.042 rows=850 loops=1)\n   ->  3 --Hash  (cost=15.20..15.20 rows=520 width=8) (actual time=0.035..0.035 rows=520 loops=1)\n         ->  4 --Seq Scan on customers c  (cost=0.00..15.20 rows=520 width=8) (actual time=0.005..0.020 rows=520 loops=1)";
    let plan = parse(input).unwrap();
    assert_eq!(plan.root.node_type, NodeType::HashJoin);
    assert_eq!(plan.root.children.len(), 2);
    assert_eq!(plan.root.children[0].node_type, NodeType::SeqScan);
    assert_eq!(plan.root.children[0].relation, Some("orders".to_string()));
    assert_eq!(plan.root.children[1].node_type, NodeType::Hash);
}

#[test]
fn unknown_node_type_is_tolerant() {
    let input = "QUERY PLAN\n----------------------------------------------------\nFrobnicate Scan on foo  (cost=0.00..1.00 rows=1 width=4)";
    let plan = parse(input).unwrap();
    assert_eq!(
        plan.root.node_type,
        NodeType::Unknown("Frobnicate Scan".to_string())
    );
    assert_eq!(plan.root.relation, Some("foo".to_string()));
    assert!(plan.root.estimated.is_some());
}

#[test]
fn cost_only_no_actual_stats() {
    let input = "QUERY PLAN\n----------------------------------------------------\nSeq Scan on t1  (cost=0.00..12.00 rows=100 width=4)\n  Filter: (status = 'active')";
    let plan = parse(input).unwrap();
    assert_eq!(plan.root.node_type, NodeType::SeqScan);
    assert!(plan.root.estimated.is_some());
    assert!(plan.root.actual.is_none());
    let est = plan.root.estimated.as_ref().unwrap();
    assert_eq!(est.plan_rows, 100.0);
    assert_eq!(est.plan_width, 4);
}

#[test]
fn never_executed_node() {
    let input = "QUERY PLAN\n----------------------------------------------------\nSeq Scan on t1  (cost=0.00..12.00 rows=100 width=4) (Actual time: never executed)";
    let plan = parse(input).unwrap();
    assert_eq!(plan.root.node_type, NodeType::SeqScan);
    let actual = plan.root.actual.as_ref().unwrap();
    assert!(!actual.executed);
    assert_eq!(actual.rows, 0.0);
    assert_eq!(actual.loops, 1.0);
    assert_eq!(actual.startup_time_ms, 0.0);
    assert_eq!(actual.total_time_ms, 0.0);
}

#[test]
fn buffer_stats_property_captured() {
    let input = "QUERY PLAN\n----------------------------------------------------\nSeq Scan on t1  (cost=0.00..12.00 rows=100 width=4) (actual time=0.015..0.052 rows=100 loops=1)\n  Buffers: shared hit=10 read=2 written=1\nTotal runtime: 0.123 ms";
    let plan = parse(input).unwrap();
    let buf_prop = plan
        .root
        .properties
        .iter()
        .find(|p| p.label == "Buffers")
        .expect("Buffers property should be captured");
    assert!(buf_prop.value.contains("shared hit=10"));
    assert!(buf_prop.value.contains("read=2"));
    assert!(plan.root.buffers.is_none());
}

#[test]
fn peak_memory_summary_and_sort_properties() {
    let input = "QUERY PLAN\n----------------------------------------------------\nSort  (cost=63.85..66.35 rows=1000 width=44) (actual time=5.432..5.876 rows=1000 loops=1)\n  Sort Key: created_at\n  Sort Method: quicksort  Memory: 77kB\nTotal runtime: 6.200 ms\nPeak Memory: 4096 kB";
    let plan = parse(input).unwrap();
    let sort_method = plan
        .root
        .properties
        .iter()
        .find(|p| p.label == "Sort Method")
        .expect("Sort Method property should be captured");
    assert!(sort_method.value.contains("quicksort"));

    let summary = plan.summary.as_ref().expect("summary should be present");
    assert_eq!(summary.total_runtime_ms, Some(6.2));
    assert_eq!(summary.peak_memory_kb, Some(4096));
}

#[test]
fn filter_property_parsed() {
    let input = "QUERY PLAN\n----------------------------------------------------\nSeq Scan on t1  (cost=0.00..12.00 rows=100 width=4) (actual time=0.015..0.052 rows=100 loops=1)\n  Filter: (status = 'active')";
    let plan = parse(input).unwrap();
    let filter = plan
        .root
        .properties
        .iter()
        .find(|p| p.label == "Filter")
        .expect("Filter property missing");
    assert_eq!(filter.value, "(status = 'active')");
}

#[test]
fn sort_key_and_sort_method() {
    let input = "QUERY PLAN\n----------------------------------------------------\nSort  (cost=63.85..66.35 rows=1000 width=44) (actual time=5.432..5.876 rows=1000 loops=1)\n  Sort Key: created_at\n  Sort Method: quicksort  Memory: 77kB";
    let plan = parse(input).unwrap();
    assert_eq!(plan.root.node_type, NodeType::Sort);
    let sort_key = plan
        .root
        .properties
        .iter()
        .find(|p| p.label == "Sort Key")
        .expect("Sort Key missing");
    assert_eq!(sort_key.value, "created_at");

    let sort_method = plan
        .root
        .properties
        .iter()
        .find(|p| p.label == "Sort Method")
        .expect("Sort Method missing");
    assert!(sort_method.value.contains("quicksort"));

    let structured = plan.root.structured_props.as_ref().unwrap();
    assert_eq!(structured.sort_method, Some("quicksort".to_string()));
}

#[test]
fn hash_cond_and_buckets() {
    let input = "QUERY PLAN\n----------------------------------------------------\nHash Join  (cost=25.38..63.85 rows=200 width=16) (actual time=0.082..0.198 rows=200 loops=1)\n   Hash Cond: (o.customer_id = c.id)\n   ->  Seq Scan on a  (cost=0.00..12.00 rows=100 width=8)\n   ->  Hash  (cost=15.20..15.20 rows=520 width=8) (actual time=0.035..0.035 rows=520 loops=1)\n         Buckets: 1024  Batches: 1  Memory Usage: 28kB";
    let plan = parse(input).unwrap();
    let hash_cond = plan
        .root
        .properties
        .iter()
        .find(|p| p.label == "Hash Cond")
        .expect("Hash Cond missing");
    assert_eq!(hash_cond.value, "(o.customer_id = c.id)");

    let hash_node = &plan.root.children[1];
    assert_eq!(hash_node.node_type, NodeType::Hash);
    let buckets = hash_node
        .properties
        .iter()
        .find(|p| p.label == "Buckets")
        .expect("Buckets property missing");
    assert!(buckets.value.contains("1024"));

    let structured = hash_node.structured_props.as_ref().unwrap();
    assert_eq!(structured.hash_buckets, Some(1024));
    assert_eq!(structured.hash_batches, Some(1));
}

#[test]
fn streaming_gather_node() {
    let input = "QUERY PLAN\n----------------------------------------------------\nStreaming(type: GATHER)  (cost=12.34..45.67 rows=500 width=28) (actual time=1.234..2.567 rows=500 loops=1)\n  Node/s: All datanodes\n  ->  Seq Scan on products  (cost=0.00..15.20 rows=500 width=28) (actual time=0.045..0.234 rows=500 loops=1)";
    let plan = parse(input).unwrap();
    assert_eq!(
        plan.root.node_type,
        NodeType::Streaming(StreamingType::Gather)
    );
    assert_eq!(plan.root.children.len(), 1);
    assert_eq!(plan.root.children[0].node_type, NodeType::SeqScan);
    assert_eq!(plan.root.children[0].relation, Some("products".to_string()));

    let node_s = plan
        .root
        .properties
        .iter()
        .find(|p| p.label == "Node/s")
        .expect("Node/s property missing");
    assert_eq!(node_s.value, "All datanodes");
}

#[test]
fn streaming_redistribute_node() {
    let input = "QUERY PLAN\n----------------------------------------------------\nStreaming(type: REDISTRIBUTE)  (cost=12.34..45.67 rows=500 width=28)\n  ->  Seq Scan on orders  (cost=0.00..15.20 rows=500 width=28)";
    let plan = parse(input).unwrap();
    assert_eq!(
        plan.root.node_type,
        NodeType::Streaming(StreamingType::Redistribute)
    );
    assert_eq!(plan.root.children.len(), 1);
    assert_eq!(plan.root.children[0].node_type, NodeType::SeqScan);
}

#[test]
fn vector_hash_join_node() {
    let input = "QUERY PLAN\n----------------------------------------------------\nVector Hash Join  (cost=25.38..63.85 rows=1000 width=32) (actual time=1.234..3.567 rows=1000 loops=1)\n   ->  Seq Scan on sales  (cost=0.00..18.50 rows=1000 width=16)\n   ->  Vector Hash Aggregate  (cost=12.00..12.00 rows=500 width=16)\n         ->  Seq Scan on items  (cost=0.00..12.00 rows=500 width=16)";
    let plan = parse(input).unwrap();
    assert_eq!(plan.root.node_type, NodeType::VectorHashJoin);
    assert_eq!(plan.root.children.len(), 2);
    assert_eq!(
        plan.root.children[1].node_type,
        NodeType::VectorHashAggregate
    );
}

#[test]
fn vector_sort_node() {
    let input = "QUERY PLAN\n----------------------------------------------------\nVector Sort  (cost=263.85..275.35 rows=5000 width=48) (actual time=48.123..52.456 rows=50000 loops=1)\n  Sort Key: created_at";
    let plan = parse(input).unwrap();
    assert_eq!(plan.root.node_type, NodeType::VectorSort);
    let sort_key = plan
        .root
        .properties
        .iter()
        .find(|p| p.label == "Sort Key")
        .expect("Sort Key missing");
    assert_eq!(sort_key.value, "created_at");
}

#[test]
fn cstore_scan_node() {
    let input = "QUERY PLAN\n----------------------------------------------------\nCStore Scan on analytics_events  (cost=0.00..45.20 rows=10000 width=64) (actual time=0.123..2.345 rows=10000 loops=1)\n  Filter: (event_date >= '2024-01-01'::date)\n  Rows Removed by Filter: 5000\n  CStore MinMax Skip: 8 from 10 CUs";
    let plan = parse(input).unwrap();
    assert_eq!(plan.root.node_type, NodeType::CStoreScan);
    assert_eq!(plan.root.relation, Some("analytics_events".to_string()));
    assert_eq!(plan.root.children.len(), 0);

    let labels: Vec<&str> = plan
        .root
        .properties
        .iter()
        .map(|p| p.label.as_str())
        .collect();
    assert!(labels.contains(&"Filter"));
    assert!(labels.contains(&"Rows Removed by Filter"));
    assert!(labels.contains(&"CStore MinMax Skip"));
}

#[test]
fn pipe_delimited_format_tolerated() {
    let input = "QUERY PLAN|\n----------------------------------------------------|\nSeq Scan on t1  (cost=0.00..12.00 rows=100 width=4)|\n  Filter: (status = 'active')|";
    let plan = parse(input).unwrap();
    assert_eq!(plan.root.node_type, NodeType::SeqScan);
    let filter = plan.root.properties.iter().find(|p| p.label == "Filter");
    assert!(filter.is_some());
}

#[test]
fn explain_analyze_total_runtime_summary() {
    let input = "QUERY PLAN\n----------------------------------------------------\nSeq Scan on t1  (cost=0.00..12.00 rows=100 width=4) (actual time=0.015..0.052 rows=100 loops=1)\n  Filter: (status = 'active')\nTotal runtime: 0.123 ms";
    let plan = parse(input).unwrap();
    let summary = plan.summary.as_ref().expect("summary should be present");
    assert_eq!(summary.total_runtime_ms, Some(0.123));
}

#[test]
fn multiple_properties_on_same_node() {
    let input = "QUERY PLAN\n----------------------------------------------------\nSeq Scan on line_items  (cost=0.00..98.50 rows=50000 width=24) (actual time=0.015..12.345 rows=500000 loops=1)\n  Filter: (created_at > '2024-01-01'::timestamp without time zone)\n  Rows Removed by Filter: 1000000\n  Peak Memory: 1024 kB";
    let plan = parse(input).unwrap();
    let labels: Vec<&str> = plan
        .root
        .properties
        .iter()
        .map(|p| p.label.as_str())
        .collect();
    assert!(labels.contains(&"Filter"));
    assert!(labels.contains(&"Rows Removed by Filter"));

    let structured = plan.root.structured_props.as_ref().unwrap();
    assert_eq!(structured.rows_removed_by_filter, Some(1000000.0));
}

#[test]
fn join_type_inner_parsed() {
    let input = "QUERY PLAN\n----------------------------------------------------\nHash Join  (cost=25.38..63.85 rows=200 width=16)\n   Hash Cond: (a.id = b.a_id)\n   ->  Seq Scan on a  (cost=0.00..12.00 rows=100 width=8)\n   ->  Hash  (cost=12.00..12.00 rows=100 width=8)\n         ->  Seq Scan on b  (cost=0.00..12.00 rows=100 width=8)";
    let plan = parse(input).unwrap();
    assert_eq!(plan.root.node_type, NodeType::HashJoin);
    assert!(plan.root.join_type.is_none());
}

#[test]
fn left_join_type_extracted_from_hash_left_join() {
    let input = "QUERY PLAN\n----------------------------------------------------\nHash Left Join  (cost=25.38..63.85 rows=200 width=16)\n   Hash Cond: (a.id = b.a_id)\n   ->  Seq Scan on a  (cost=0.00..12.00 rows=100 width=8)\n   ->  Hash  (cost=12.00..12.00 rows=100 width=8)\n         ->  Seq Scan on b  (cost=0.00..12.00 rows=100 width=8)";
    let plan = parse(input).unwrap();
    assert_eq!(plan.root.node_type, NodeType::Hash);
    assert_eq!(
        plan.root.join_type,
        Some(ogexplain_core::model::JoinType::Left)
    );
}

#[test]
fn nested_loop_with_actual_rows_only() {
    let input = "QUERY PLAN\n----------------------------------------------------\nNested Loop  (cost=0.29..16.61 rows=5 width=136) (actual rows=3 loops=1)\n   ->  Index Scan using idx_a on a  (cost=0.29..8.30 rows=1 width=68) (actual rows=1 loops=1)\n   ->  Index Scan using idx_b on b  (cost=0.29..8.30 rows=5 width=68) (actual rows=3 loops=1)";
    let plan = parse(input).unwrap();
    assert_eq!(plan.root.node_type, NodeType::NestedLoop);
    let actual = plan.root.actual.as_ref().unwrap();
    assert_eq!(actual.rows, 3.0);
    assert_eq!(actual.loops, 1.0);
    assert!(actual.executed);
    assert_eq!(actual.startup_time_ms, 0.0);
    assert_eq!(actual.total_time_ms, 0.0);
}

#[test]
fn parse_multi_two_plans() {
    let input = "QUERY PLAN\n----------------------------------------------------\nSeq Scan on t1  (cost=0.00..12.00 rows=100 width=4)\n\n-- second plan\nQUERY PLAN\n----------------------------------------------------\nSeq Scan on t2  (cost=0.00..15.00 rows=200 width=8)";
    let plans = parse_multi(input).unwrap();
    assert_eq!(plans.len(), 2);
    assert_eq!(plans[0].root.node_type, NodeType::SeqScan);
    assert_eq!(plans[0].root.relation, Some("t1".to_string()));
    assert_eq!(plans[1].root.node_type, NodeType::SeqScan);
    assert_eq!(plans[1].root.relation, Some("t2".to_string()));
}

#[test]
fn whitespace_only_errors() {
    let result = parse("   \n\t\n   ");
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("Empty input"),
        "expected 'Empty input', got: {}",
        err
    );
}

#[test]
fn no_plan_nodes_errors() {
    let result = parse("QUERY PLAN\n----------------------------------------------------\n");
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("No plan nodes") || err.contains("Empty input"),
        "expected plan-related error, got: {}",
        err
    );
}

// --- auto_explain format tests (enable_auto_explain=on, auto_explain_level=notice) ---

#[test]
fn auto_explain_single_function_scan() {
    let input = r#"NOTICE:  duration: 6.749 ms  plan:
Query Text: SELECT * FROM gaussdb.fn_get_team_members(1)
Function Scan on fn_get_team_members  (cost=0.25..10.25 rows=1000 width=68) (actual time=6.646..6.646 rows=0 loops=1)
  (Buffers: shared hit=131 read=8)
Total runtime: 6.749 ms"#;

    let plan = parse(input).expect("auto_explain parse failed");
    assert_eq!(plan.root.node_type, NodeType::FunctionScan);
    assert_eq!(plan.root.children.len(), 0);
    // "Total runtime:" should be parsed as summary
    assert!(plan.summary.is_some());
    let summary = plan.summary.as_ref().unwrap();
    assert!(summary.total_runtime_ms.is_some());
    // Buffers line should be attached as a property
    assert!(!plan.root.properties.is_empty());
}

#[test]
fn auto_explain_with_timestamp_prefix() {
    let input = r#"2025-07-27 10:00:00.123 CST [12345] NOTICE:  duration: 6.749 ms  plan:
Query Text: SELECT * FROM gaussdb.fn_get_team_members(1)
Function Scan on fn_get_team_members  (cost=0.25..10.25 rows=1000 width=68)
Total runtime: 6.749 ms"#;

    let plan = parse(input).expect("parse with timestamp failed");
    assert_eq!(plan.root.node_type, NodeType::FunctionScan);
    assert!(plan.summary.is_some());
}

#[test]
fn auto_explain_nested_plan_with_sort() {
    let input = r#"NOTICE:  duration: 10.234 ms  plan:
Query Text: SELECT e.emp_name, e.base_salary FROM gaussdb.employees e WHERE e.dept_id = 1 ORDER BY e.base_salary DESC
Sort  (cost=2.04..2.05 rows=1 width=46) (actual time=10.100..10.100 rows=5 loops=1)
  Sort Key: e.base_salary DESC
  ->  Seq Scan on employees e  (cost=0.00..1.03 rows=1 width=46) (actual time=0.050..10.050 rows=5 loops=1)
        Filter: (dept_id = 1)
Total runtime: 10.234 ms"#;

    let plan = parse(input).expect("nested auto_explain parse failed");
    assert_eq!(plan.root.node_type, NodeType::Sort);
    assert_eq!(plan.root.relation, None);
    assert_eq!(plan.root.children.len(), 1);
    assert_eq!(plan.root.children[0].node_type, NodeType::SeqScan);
    assert_eq!(
        plan.root.children[0].relation,
        Some("employees".to_string())
    );
    // properties of the Seq Scan child
    assert!(plan.root.children[0]
        .properties
        .iter()
        .any(|p| p.label == "Filter"));
    assert!(plan.summary.is_some());
}

#[test]
fn auto_explain_missing_query_text() {
    let input = r#"NOTICE:  duration: 3.000 ms  plan:
Seq Scan on employees  (cost=0.00..1.03 rows=100 width=46)
Total runtime: 3.000 ms"#;

    let plan = parse(input).expect("missing query text parse failed");
    assert_eq!(plan.root.node_type, NodeType::SeqScan);
    assert!(plan.summary.is_some());
}

#[test]
fn auto_explain_with_actual_runtime_only() {
    let input = r#"NOTICE:  duration: 5.000 ms  plan:
Query Text: SELECT * FROM t WHERE id = 1
Seq Scan on t  (cost=0.00..10.00 rows=5 width=36) (actual time=0.100..4.500 rows=5 loops=1)
Total runtime: 5.000 ms"#;

    let plan = parse(input).expect("parse with actual runtime failed");
    assert_eq!(plan.root.node_type, NodeType::SeqScan);
    assert!(plan.root.actual.is_some());
    let actual = plan.root.actual.as_ref().unwrap();
    assert!(actual.executed);
    assert_eq!(actual.rows, 5.0);
    assert!(plan.summary.is_some());
    let summary = plan.summary.as_ref().unwrap();
    assert_eq!(summary.total_runtime_ms, Some(5.0));
}

#[test]
fn auto_explain_vector_node() {
    let input = r#"NOTICE:  duration: 2.345 ms  plan:
Query Text: SELECT COUNT(*) FROM big_table
Vector Hash Aggregate  (cost=100.00..100.05 rows=1 width=8) (actual time=2.000..2.100 rows=1 loops=1)
  ->  Vec Sort  (cost=0.00..100.00 rows=1000 width=0)
        Sort Key: 1
        ->  CStore Scan on big_table  (cost=0.00..50.00 rows=1000 width=0)
Total runtime: 2.345 ms"#;

    let plan = parse(input).expect("vector auto_explain parse failed");
    assert_eq!(plan.root.node_type, NodeType::VectorHashAggregate);
    assert_eq!(plan.root.children.len(), 1);
}

#[test]
fn auto_explain_multi_query_parse_multi() {
    let input = r#"NOTICE:  duration: 6.749 ms  plan:
Query Text: SELECT * FROM gaussdb.fn_get_team_members(1)
Function Scan on fn_get_team_members  (cost=0.25..10.25 rows=1000 width=68)
Total runtime: 6.749 ms
NOTICE:  duration: 10.234 ms  plan:
Query Text: SELECT * FROM gaussdb.employees WHERE dept_id = 1
Seq Scan on employees  (cost=0.00..1.03 rows=100 width=46) (actual time=0.050..10.050 rows=5 loops=1)
  Filter: (dept_id = 1)
Total runtime: 10.234 ms"#;

    let plans = parse_multi(input).expect("multi auto_explain parse failed");
    assert_eq!(plans.len(), 2, "should extract 2 plans");

    // First plan: Function Scan
    assert_eq!(plans[0].root.node_type, NodeType::FunctionScan);
    assert!(plans[0].summary.is_some());
    assert_eq!(
        plans[0].summary.as_ref().unwrap().total_runtime_ms,
        Some(6.749)
    );

    // Second plan: Seq Scan with child properties
    assert_eq!(plans[1].root.node_type, NodeType::SeqScan);
    assert!(plans[1].root.actual.is_some());
    assert!(plans[1].summary.is_some());
    assert_eq!(
        plans[1].summary.as_ref().unwrap().total_runtime_ms,
        Some(10.234)
    );
}

// --- auto_explain with stored procedure / cursor / loop scenarios ---

#[test]
fn auto_explain_proc_with_cursor_loop() {
    // fn_pipe_emp_list has: FOR v_rec IN (SELECT ... FROM employees ...) LOOP ... RETURN QUERY ...
    let input = r#"NOTICE:  duration: 3.826 ms  plan:
Query Text: SELECT * FROM gaussdb.fn_pipe_emp_list(1)
Function Scan on fn_pipe_emp_list  (cost=0.25..10.25 rows=1000 width=68) (actual time=3.655..3.655 rows=0 loops=1)
  (Buffers: shared hit=35)
Total runtime: 3.826 ms"#;

    let plan = parse(input).expect("proc cursor parse failed");
    assert_eq!(plan.root.node_type, NodeType::FunctionScan);
    assert_eq!(plan.root.relation, Some("fn_pipe_emp_list".to_string()));
    assert!(plan.root.actual.is_some());
    assert!(plan.summary.is_some());
}

#[test]
fn auto_explain_proc_with_if_elsif_logic() {
    // fn_get_tax_rate has: IF salary <= 5000 ... ELSIF ... ELSE ...
    let input = r#"NOTICE:  duration: 1.774 ms  plan:
Query Text: SELECT * FROM gaussdb.fn_get_tax_rate(15000)
Function Scan on fn_get_tax_rate  (cost=0.25..0.26 rows=1 width=32) (actual time=1.738..1.738 rows=1 loops=1)
  (Buffers: shared hit=64)
Total runtime: 1.774 ms"#;

    let plan = parse(input).expect("proc if-else parse failed");
    assert_eq!(plan.root.node_type, NodeType::FunctionScan);
    assert!(plan.root.actual.is_some());
    assert_eq!(plan.root.actual.as_ref().unwrap().rows, 1.0);
    assert!(plan.summary.is_some());
}

#[test]
fn auto_explain_multi_function_calls_result_node() {
    // SELECT fn_dept_avg_salary(1), fn_calc_years_of_service(...) → Result node
    let input = r#"NOTICE:  duration: 7.032 ms  plan:
Query Text: SELECT gaussdb.fn_dept_avg_salary(1), gaussdb.fn_calc_years_of_service('2020-01-01'::timestamp)
Result  (cost=0.00..0.51 rows=1 width=0) (actual time=6.987..6.987 rows=1 loops=1)
Total runtime: 7.032 ms"#;

    let plan = parse(input).expect("result node parse failed");
    assert_eq!(plan.root.node_type, NodeType::Result);
    assert!(plan.root.actual.is_some());
    assert!(plan.summary.is_some());
}

#[test]
fn auto_explain_proc_internal_sql_leaked() {
    // Simulates auto_explain capturing a long-running internal SQL inside a stored procedure loop.
    // The function call itself + the internal heavy query both appear in the log.
    let input = r#"NOTICE:  duration: 2.100 ms  plan:
Query Text: SELECT * FROM gaussdb.fn_pipe_emp_list(1)
Function Scan on fn_pipe_emp_list  (cost=0.25..10.25 rows=1000 width=68)
Total runtime: 2.100 ms
NOTICE:  duration: 150.500 ms  plan:
Query Text: SELECT emp_id, emp_name, fn_format_salary(base_salary * (1 + bonus_pct)) FROM gaussdb.employees WHERE dept_id = 1 AND status = 'ACTIVE' ORDER BY base_salary DESC
Sort  (cost=5.50..5.51 rows=1 width=100) (actual time=100.000..150.000 rows=500 loops=1)
  Sort Key: base_salary DESC
  ->  Seq Scan on employees  (cost=0.00..5.49 rows=1 width=100) (actual time=0.050..50.000 rows=500 loops=1)
        Filter: ((dept_id = 1) AND (status = 'ACTIVE'::text))
        Rows Removed by Filter: 9500
Total runtime: 150.500 ms"#;

    let plans = parse_multi(input).expect("proc internal sql parse failed");
    assert_eq!(plans.len(), 2);

    // First: Function Scan wrapper
    assert_eq!(plans[0].root.node_type, NodeType::FunctionScan);
    assert_eq!(plans[0].root.children.len(), 0);

    // Second: internal heavy query with full tree
    assert_eq!(plans[1].root.node_type, NodeType::Sort);
    assert_eq!(plans[1].root.children.len(), 1);
    assert_eq!(plans[1].root.children[0].node_type, NodeType::SeqScan);
    assert_eq!(
        plans[1].root.children[0].relation,
        Some("employees".to_string())
    );
    assert!(plans[1].root.children[0]
        .properties
        .iter()
        .any(|p| p.label == "Rows Removed by Filter"));
    assert!(plans[1].summary.is_some());
    assert_eq!(
        plans[1].summary.as_ref().unwrap().total_runtime_ms,
        Some(150.5)
    );
}

#[test]
fn auto_explain_recursive_function() {
    // fn_factorial calls itself recursively
    let input = r#"NOTICE:  duration: 0.345 ms  plan:
Query Text: SELECT * FROM gaussdb.fn_factorial(10)
Function Scan on fn_factorial  (cost=0.25..0.26 rows=1 width=4) (actual time=0.200..0.300 rows=1 loops=1)
Total runtime: 0.345 ms"#;

    let plan = parse(input).expect("recursive func parse failed");
    assert_eq!(plan.root.node_type, NodeType::FunctionScan);
    assert!(plan.root.actual.is_some());
    assert!(plan.summary.is_some());
}
