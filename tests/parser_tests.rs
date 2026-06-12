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
