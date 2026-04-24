use ogexplain_core::summary::{ComplexityInput, PushdownStatus, SummaryRow};

#[test]
fn summary_row_from_simple_plan() {
    let input = "\
Seq Scan on t1  (cost=0.00..12.00 rows=100 width=4) (actual time=0.015..0.052 rows=100 loops=1)
  Filter: (status = 'active')
  Rows Removed by Filter: 50
Total runtime: 0.089 ms";
    let plan = ogexplain_core::parse(input).unwrap();
    let diag = ogexplain_core::analyze(&plan);
    let row = SummaryRow::compute(&plan, &diag, None);
    assert_eq!(row.tables, 0);
    assert_eq!(row.joins, 0);
    assert_eq!(row.subqueries, 0);
    assert!(row.score.is_none());
    assert!(row.total_cost > 0.0);
    assert!(row.total_time_ms > 0.0);
    assert_eq!(row.actual_rows, Some(100.0));
    assert_eq!(row.critical_count, 0);
    assert_eq!(row.warning_count, 0);
}

#[test]
fn summary_row_with_complexity() {
    let input = "\
Hash Join  (cost=25.38..63.85 rows=200 width=16) (actual time=0.082..0.198 rows=200 loops=1)
   Hash Cond: (o.customer_id = c.id)
   ->  Seq Scan on orders o  (cost=0.00..18.50 rows=850 width=12) (actual time=0.008..0.042 rows=850 loops=1)
   ->  Hash  (cost=1.10..1.10 rows=10 width=4) (actual time=0.008..0.008 rows=10 loops=1)
         ->  Seq Scan on users u  (cost=0.00..1.00 rows=10 width=4) (actual time=0.005..0.005 rows=10 loops=1)
Total runtime: 0.350 ms";
    let plan = ogexplain_core::parse(input).unwrap();
    let diag = ogexplain_core::analyze(&plan);
    // Construct ComplexityInput manually — ogexplain-core is decoupled from ogsql-complexity.
    // For the SQL "SELECT u.id FROM users u JOIN orders o ON u.id = o.user_id WHERE u.age > 18":
    //   tables=2 (users, orders), joins=1, subqueries=0, score≈4.0 (Gauss weights), level=Simple
    let sql_text = "SELECT u.id FROM users u JOIN orders o ON u.id = o.user_id WHERE u.age > 18";
    let complexity = ComplexityInput {
        sql_preview: Some(sql_text.lines().next().unwrap_or("").to_string()),
        tables: 2,
        joins: 1,
        subqueries: 0,
        where_conditions: 0,
        aggregates: 0,
        cases: 0,
        set_ops: 0,
        ctes: 0,
        windows: 0,
        has_group_by: false,
        has_order_by: false,
        has_distinct: false,
        subquery_depth: 0,
        hints: 0,
        score: Some(4.0),
        level: Some("Simple".to_string()),
        gauss_score: None,
        gauss_level: None,
        sql_category: None,
        sql_sub_type: None,
        gauss_sql_structure: None,
        gauss_pl_logic: None,
        gauss_advanced_feature: None,
        gauss_extension: None,
        gauss_tags: vec![],
    };
    let row = SummaryRow::compute(&plan, &diag, Some(&complexity));

    assert_eq!(row.tables, 2);
    assert_eq!(row.joins, 1);
    assert!(row.score.unwrap() > 0.0);
    assert!(row.total_time_ms > 0.0);
    assert_eq!(row.actual_rows, Some(200.0));
}

#[test]
fn summary_row_estimation_ratio() {
    let input = "\
Sort  (cost=263.85..275.35 rows=5000 width=48) (actual time=48.123..52.456 rows=50000 loops=1)
  Sort Key: l.created_at
  ->  Seq Scan on line_items  (cost=0.00..98.50 rows=50000 width=24) (actual time=0.015..12.345 rows=500000 loops=1)
Total runtime: 55.789 ms";
    let plan = ogexplain_core::parse(input).unwrap();
    let diag = ogexplain_core::analyze(&plan);
    let row = SummaryRow::compute(&plan, &diag, None);
    // Root: est 5000 vs actual 50000 = 10x
    // SeqScan: est 50000 vs actual 500000 = 10x
    let ratio = row.worst_est_ratio.unwrap();
    assert!(
        ratio >= 9.0 && ratio <= 11.0,
        "expected ~10x, got {}",
        ratio
    );
}

#[test]
fn summary_row_spill_detection() {
    let input = "\
Sort  (cost=63.85..66.35 rows=1000 width=44) (actual time=5.432..5.876 rows=1000 loops=1)
  Sort Key: created_at
  Sort Method: external merge  Disk: 48kB
Total runtime: 6.200 ms";
    let plan = ogexplain_core::parse(input).unwrap();
    let diag = ogexplain_core::analyze(&plan);
    let row = SummaryRow::compute(&plan, &diag, None);
    assert!(
        row.spill_kb.unwrap() > 0.0,
        "expected spill > 0, got {:?}",
        row.spill_kb
    );
}

#[test]
fn summary_row_pushdown_status() {
    let input = "\
Streaming(type: GATHER)  (cost=12.34..45.67 rows=500 width=28) (actual time=1.234..2.567 rows=500 loops=1)
  Node/s: All datanodes
  ->  Seq Scan on products  (cost=0.00..15.20 rows=500 width=28) (actual time=0.045..0.234 rows=500 loops=1)
Total runtime: 3.000 ms";
    let plan = ogexplain_core::parse(input).unwrap();
    let diag = ogexplain_core::analyze(&plan);
    let row = SummaryRow::compute(&plan, &diag, None);
    assert_eq!(row.pushdown, PushdownStatus::NotPushed);
}

#[test]
fn summary_row_finding_counts() {
    let input = "\
Sort  (cost=263.85..275.35 rows=5000 width=48) (actual time=48.123..52.456 rows=50000 loops=1)
  Sort Key: l.created_at
  Sort Method: external merge  Disk: 5840kB
  ->  Seq Scan on line_items  (cost=0.00..98.50 rows=50000 width=24) (actual time=0.015..12.345 rows=500000 loops=1)
        Filter: (created_at > '2024-01-01'::timestamp without time zone)
        Rows Removed by Filter: 1000000
Total runtime: 55.789 ms
Peak Memory: 8192 kB";
    let plan = ogexplain_core::parse(input).unwrap();
    let diag = ogexplain_core::analyze(&plan);
    let row = SummaryRow::compute(&plan, &diag, None);
    assert!(row.critical_count > 0, "expected critical findings");
    assert!(row.warning_count > 0, "expected warning findings");
    assert!(row.peak_memory_kb.unwrap() > 0.0, "expected peak memory");
}
