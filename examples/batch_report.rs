use ogexplain_core::{parse_multi, analyze};
use std::fs;
use std::collections::HashMap;

fn main() {
    let dir = "examples/gauss";
    let entries: Vec<_> = fs::read_dir(dir).unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| {
            let ext = e.path().extension().and_then(|e| e.to_str()).unwrap_or("");
            ext == "source" || ext == "out"
        })
        .collect();

    let mut total_files = 0usize;
    let mut total_blocks = 0usize;
    let mut ok_files = 0usize;
    let mut no_blocks = 0usize;
    let mut parse_err = 0usize;
    let mut parse_errors: Vec<(String, String)> = Vec::new();
    let mut no_block_files: Vec<String> = Vec::new();

    // Per-node-type stats
    let mut node_type_counts: HashMap<String, usize> = HashMap::new();
    let mut total_nodes = 0usize;

    // Diagnostic stats
    let mut total_findings = 0usize;
    let mut findings_by_severity: HashMap<String, usize> = HashMap::new();
    let mut findings_by_rule: HashMap<String, usize> = HashMap::new();

    for entry in &entries {
        let path = entry.path();
        total_files += 1;
        let content = match fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => { no_blocks += 1; continue; }
        };
        let name = path.file_name().unwrap().to_str().unwrap().to_string();
        match parse_multi(&content) {
            Ok(plans) => {
                if plans.is_empty() {
                    no_blocks += 1;
                    no_block_files.push(name);
                } else {
                    ok_files += 1;
                    total_blocks += plans.len();
                    for plan in &plans {
                        // Count node types
                        fn count_nodes(node: &ogexplain_core::PlanNode, counts: &mut HashMap<String, usize>, total: &mut usize) {
                            *total += 1;
                            *counts.entry(format!("{:?}", node.node_type)).or_insert(0) += 1;
                            for child in &node.children {
                                count_nodes(child, counts, total);
                            }
                        }
                        count_nodes(&plan.root, &mut node_type_counts, &mut total_nodes);

                        // Run diagnostics
                        let report = analyze(plan);
                        total_findings += report.findings.len();
                        for f in &report.findings {
                            *findings_by_severity.entry(format!("{:?}", f.severity)).or_insert(0) += 1;
                            *findings_by_rule.entry(f.rule_id.clone()).or_insert(0) += 1;
                        }
                    }
                }
            }
            Err(e) => {
                parse_err += 1;
                parse_errors.push((name, format!("{}", e)));
            }
        }
    }

    println!("=== 批量解析与诊断分析报告 ===\n");
    println!("--- 1. 文件解析统计 ---");
    println!("扫描目录:         examples/gauss/");
    println!("总文件数:         {}", total_files);
    println!("成功解析:         {} ({:.1}%)", ok_files, ok_files as f64 / total_files as f64 * 100.0);
    println!("解析 EXPLAIN 块:  {}", total_blocks);
    println!("无 EXPLAIN 块:    {} ({:.1}%)", no_blocks, no_blocks as f64 / total_files as f64 * 100.0);
    println!("解析失败:         {} ({:.1}%)", parse_err, parse_err as f64 / total_files as f64 * 100.0);
    println!("解析总节点数:     {}", total_nodes);

    println!("\n--- 2. 无 EXPLAIN 块的文件 ---");
    for f in &no_block_files { println!("  {}", f); }

    if !parse_errors.is_empty() {
        println!("\n--- 3. 解析失败详情 ---");
        for (name, err) in &parse_errors { println!("  {}: {}", name, err); }
    }

    println!("\n--- 4. 节点类型分布 (Top 30) ---");
    let mut sorted_nodes: Vec<_> = node_type_counts.iter().collect();
    sorted_nodes.sort_by(|a, b| b.1.cmp(a.1));
    for (i, (nt, count)) in sorted_nodes.iter().take(30).enumerate() {
        println!("  {:2}. {:40s} {:>6} ({:>5.1}%)", i+1, nt, count, **count as f64 / total_nodes as f64 * 100.0);
    }
    println!("  ... 共 {} 种节点类型", sorted_nodes.len());

    println!("\n--- 5. 诊断发现统计 ---");
    println!("总发现数: {}", total_findings);
    let mut sorted_sev: Vec<_> = findings_by_severity.iter().collect();
    sorted_sev.sort_by(|a, b| b.1.cmp(a.1));
    for (sev, count) in &sorted_sev {
        println!("  {:10s}: {} ({:.1}%)", sev, count, **count as f64 / total_findings.max(1) as f64 * 100.0);
    }

    println!("\n--- 6. 规则命中分布 ---");
    let mut sorted_rules: Vec<_> = findings_by_rule.iter().collect();
    sorted_rules.sort_by(|a, b| b.1.cmp(a.1));
    for (rule_id, count) in &sorted_rules {
        println!("  {:10s}: {} ({:.1}%)", rule_id, count, **count as f64 / total_findings.max(1) as f64 * 100.0);
    }
}
