use ogexplain_core::analyzer::config::DiagnosticConfig;
use ogexplain_core::session::{analyze_session, BottleneckKind};
use ogexplain_core::{parse, parse_multi};

fn parse_entry(query: &str, explain: &str) -> (String, ogexplain_core::model::ExplainPlan) {
    let plan = parse(explain).unwrap_or_else(|e| panic!("parse failed for '{query}': {e}"));
    (query.to_string(), plan)
}

#[test]
fn empty_session() {
    let session = analyze_session(&[], &DiagnosticConfig::default());
    assert_eq!(session.total_entries, 0);
    assert_eq!(session.total_time_ms, 0.0);
    assert!(session.serial_bottlenecks.is_empty());
    assert!(session.template_groups.is_empty());
}

#[test]
fn single_entry_session() {
    let entries = vec![parse_entry(
        "SELECT * FROM t",
        "Seq Scan on t  (cost=0.00..10.00 rows=5 width=36) (actual time=0.100..4.500 rows=5 loops=1)\nTotal runtime: 5.000 ms",
    )];
    let session = analyze_session(&entries, &DiagnosticConfig::default());
    assert_eq!(session.total_entries, 1);
    assert!((session.total_time_ms - 5.0).abs() < 0.01);
    assert_eq!(session.serial_bottlenecks.len(), 1);
    assert_eq!(
        session.serial_bottlenecks[0].bottleneck_kind,
        BottleneckKind::Primary
    );
    assert!(session.template_groups.is_empty());
}

#[test]
fn serial_bottleneck_detection() {
    let entries = vec![
        parse_entry(
            "UPDATE orders SET status = 'processing'",
            "Update on orders  (cost=0.00..1.00 rows=1 width=4) (actual time=0.100..2.000 rows=1 loops=1)\nTotal runtime: 2.100 ms",
        ),
        parse_entry(
            "SELECT inventory_qty FROM inventory",
            "Seq Scan on inventory  (cost=0.00..1.00 rows=1 width=4) (actual time=0.050..0.400 rows=1 loops=1)\nTotal runtime: 0.500 ms",
        ),
        parse_entry(
            "SELECT SUM(price * qty) FROM order_items oi JOIN products p ON oi.product_id = p.product_id",
            "Hash Join  (cost=10.00..100.00 rows=1000 width=8) (actual time=50.000..350.000 rows=1000 loops=1)\n  Hash Cond: (oi.product_id = p.product_id)\n  ->  Seq Scan on order_items oi  (cost=0.00..50.00 rows=1000 width=12) (actual time=0.050..100.000 rows=1000 loops=1)\n  ->  Hash  (cost=5.00..5.00 rows=100 width=8)\n        ->  Seq Scan on products p  (cost=0.00..5.00 rows=100 width=8) (actual time=0.010..10.000 rows=100 loops=1)\nTotal runtime: 350.200 ms",
        ),
        parse_entry(
            "INSERT INTO audit_log VALUES (...)",
            "Insert on audit_log  (cost=0.00..0.01 rows=1 width=0) (actual time=0.100..1.000 rows=0 loops=1)\nTotal runtime: 1.200 ms",
        ),
    ];

    let session = analyze_session(&entries, &DiagnosticConfig::default());
    assert_eq!(session.total_entries, 4);
    assert!((session.total_time_ms - 354.0).abs() < 1.0);

    let primary: Vec<_> = session
        .serial_bottlenecks
        .iter()
        .filter(|b| b.bottleneck_kind == BottleneckKind::Primary)
        .collect();
    assert_eq!(primary.len(), 1);
    assert_eq!(primary[0].step_index, 2);
    assert!(primary[0].contribution_pct > 90.0);
}

#[test]
fn loop_template_grouping() {
    let plan_text = "Seq Scan on orders  (cost=0.00..50.00 rows=100 width=4) (actual time=0.050..5.000 rows=100 loops=1)\n  Filter: (customer_id = 1)\nTotal runtime: 5.200 ms";

    let entries: Vec<_> = (1..=5)
        .map(|i| {
            parse_entry(
                &format!("SELECT COUNT(*) FROM orders WHERE customer_id = {i}"),
                plan_text,
            )
        })
        .collect();

    let session = analyze_session(&entries, &DiagnosticConfig::default());
    assert_eq!(session.total_entries, 5);
    assert_eq!(session.template_groups.len(), 1);

    let group = &session.template_groups[0];
    assert_eq!(group.count, 5);
    assert!((group.cum_time_ms - 26.0).abs() < 0.1);
    assert!((group.avg_time_ms - 5.2).abs() < 0.01);
    assert_eq!(group.root_op, "SeqScan");
    assert!((group.degradation_ratio - 1.0).abs() < 0.01);
}

#[test]
fn mixed_templates_and_unique_queries() {
    let fts = "Seq Scan on orders  (cost=0.00..50.00 rows=100 width=4) (actual time=0.050..5.000 rows=100 loops=1)\nTotal runtime: 5.200 ms";
    let idx = "Index Scan using pk on inventory  (cost=0.00..1.00 rows=1 width=4) (actual time=0.010..0.050 rows=1 loops=1)\nTotal runtime: 0.100 ms";

    let entries = vec![
        parse_entry("SELECT * FROM orders WHERE customer_id = 1", fts),
        parse_entry("SELECT * FROM inventory WHERE id = 1", idx),
        parse_entry("SELECT * FROM orders WHERE customer_id = 2", fts),
        parse_entry("SELECT * FROM inventory WHERE id = 2", idx),
        parse_entry("SELECT * FROM orders WHERE customer_id = 3", fts),
        parse_entry(
            "INSERT INTO audit_log VALUES (1)",
            "Insert on audit_log  (cost=0.00..0.01 rows=1 width=0) (actual time=0.100..1.000 rows=0 loops=1)\nTotal runtime: 1.200 ms",
        ),
    ];

    let session = analyze_session(&entries, &DiagnosticConfig::default());
    assert_eq!(session.total_entries, 6);
    assert_eq!(session.template_groups.len(), 2);

    let top = &session.template_groups[0];
    assert_eq!(top.count, 3);
    assert_eq!(top.root_op, "SeqScan");
    assert!((top.cum_time_ms - 15.6).abs() < 0.1);

    let second = &session.template_groups[1];
    assert_eq!(second.count, 2);
    assert_eq!(second.root_op, "IndexScan");
}

#[test]
fn auto_explain_proc_internal_sql_session() {
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

    let plans = parse_multi(input).expect("parse_multi failed");
    assert_eq!(plans.len(), 2);

    let entries: Vec<_> = plans
        .into_iter()
        .enumerate()
        .map(|(i, p)| (format!("query_{i}"), p))
        .collect();

    let session = analyze_session(&entries, &DiagnosticConfig::default());
    assert_eq!(session.total_entries, 2);
    assert!((session.total_time_ms - 152.6).abs() < 0.1);

    let primary: Vec<_> = session
        .serial_bottlenecks
        .iter()
        .filter(|b| b.bottleneck_kind == BottleneckKind::Primary)
        .collect();
    assert_eq!(primary.len(), 1);
    assert_eq!(primary[0].step_index, 1);
    assert!(primary[0].contribution_pct > 90.0);

    assert!(
        !primary[0].diagnostic.findings.is_empty(),
        "internal SQL should have diagnostic findings"
    );
}

#[test]
fn degradation_ratio_detection() {
    let fast = "Seq Scan on t  (cost=0.00..10.00 rows=5 width=36) (actual time=0.100..1.000 rows=5 loops=1)\nTotal runtime: 1.000 ms";
    let slow = "Seq Scan on t  (cost=0.00..10.00 rows=5 width=36) (actual time=0.100..10.000 rows=5 loops=1)\nTotal runtime: 10.000 ms";

    let entries = vec![
        parse_entry("SELECT * FROM t WHERE id = 1", fast),
        parse_entry("SELECT * FROM t WHERE id = 2", fast),
        parse_entry("SELECT * FROM t WHERE id = 3", slow),
        parse_entry("SELECT * FROM t WHERE id = 4", fast),
    ];

    let session = analyze_session(&entries, &DiagnosticConfig::default());
    assert_eq!(session.template_groups.len(), 1);

    let group = &session.template_groups[0];
    assert_eq!(group.count, 4);
    assert!((group.cum_time_ms - 13.0).abs() < 0.1);
    assert!((group.min_time_ms - 1.0).abs() < 0.01);
    assert!((group.max_time_ms - 10.0).abs() < 0.01);
    assert!(
        group.degradation_ratio > 3.0,
        "degradation_ratio should be > 3, got {}",
        group.degradation_ratio
    );
}

#[test]
fn contribution_percentages_sum_to_100() {
    let entries = vec![
        parse_entry(
            "q1",
            "Seq Scan on a  (cost=0.00..1.00 rows=1 width=4) (actual time=0.1..1.0 rows=1 loops=1)\nTotal runtime: 10.000 ms",
        ),
        parse_entry(
            "q2",
            "Seq Scan on b  (cost=0.00..1.00 rows=1 width=4) (actual time=0.1..2.0 rows=1 loops=1)\nTotal runtime: 20.000 ms",
        ),
        parse_entry(
            "q3",
            "Seq Scan on c  (cost=0.00..1.00 rows=1 width=4) (actual time=0.1..3.0 rows=1 loops=1)\nTotal runtime: 30.000 ms",
        ),
        parse_entry(
            "q4",
            "Seq Scan on d  (cost=0.00..1.00 rows=1 width=4) (actual time=0.1..4.0 rows=1 loops=1)\nTotal runtime: 40.000 ms",
        ),
    ];

    let session = analyze_session(&entries, &DiagnosticConfig::default());
    let total_pct: f64 = session
        .serial_bottlenecks
        .iter()
        .map(|b| b.contribution_pct)
        .sum();
    assert!(
        (total_pct - 100.0).abs() < 0.01,
        "contributions should sum to 100%, got {total_pct}"
    );
}
