use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use colored::*;
use ogexplain_core::suggester::SuggestionEngine;
use std::io::{self, Read};

#[derive(Parser)]
#[command(name = "ogexplain")]
#[command(about = "OpenGauss EXPLAIN plan analyzer")]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    #[command(about = "Analyze an EXPLAIN output file")]
    Analyze {
        #[arg(help = "Path to EXPLAIN output file, or '-' for stdin")]
        file: String,
        #[arg(short, long, default_value = "text", help = "Output format")]
        output: String,
        #[arg(long, default_value = "info", help = "Minimum severity threshold")]
        threshold: String,
        #[arg(short, long, help = "Only show findings, no summary")]
        quiet: bool,
        #[arg(short, long, help = "Verbose output")]
        verbose: bool,
        #[arg(long, help = "Parse all EXPLAIN blocks in file")]
        multi: bool,
    },
}

pub fn run() -> Result<()> {
    let cli = Cli::parse();

    let cmd = cli.command.unwrap_or(Commands::Analyze {
        file: "-".to_string(),
        output: "text".to_string(),
        threshold: "info".to_string(),
        quiet: false,
        verbose: false,
        multi: false,
    });

    match cmd {
        Commands::Analyze {
            file,
            output,
            threshold,
            quiet,
            verbose: _,
            multi: _,
        } => {
            let input = read_input(&file)?;

            let blocks = ogexplain_core::sql::segment_input(&input);

            if blocks.is_empty() {
                let plan =
                    ogexplain_core::parse(&input).context("Failed to parse EXPLAIN output")?;
                let complexity = try_complexity(&input);
                output_block(&plan, &output, &threshold, quiet, complexity.as_ref(), 1, 1)?;
            } else if blocks.len() == 1 {
                let block = &blocks[0];
                let plan = ogexplain_core::parse(&block.explain_text)
                    .context("Failed to parse EXPLAIN output")?;
                let complexity = block
                    .sql_text
                    .as_ref()
                    .and_then(|sql| ogsql_complexity::analyze(sql).ok());
                output_block(&plan, &output, &threshold, quiet, complexity.as_ref(), 1, 1)?;
            } else {
                for (i, block) in blocks.iter().enumerate() {
                    let num = i + 1;
                    let total = blocks.len();
                    if let Ok(plan) = ogexplain_core::parse(&block.explain_text) {
                        let complexity = block
                            .sql_text
                            .as_ref()
                            .and_then(|sql| ogsql_complexity::analyze(sql).ok());
                        output_block(
                            &plan,
                            &output,
                            &threshold,
                            quiet,
                            complexity.as_ref(),
                            num,
                            total,
                        )?;
                    } else if let Some(sql) = &block.sql_text {
                        output_sql_only(sql, &output, num, total)?;
                    }
                }
            }
        }
    }

    Ok(())
}

fn try_complexity(input: &str) -> Option<ogsql_complexity::ComplexityReport> {
    let extracted = ogexplain_core::sql::ExtractedContent::from_text(input);
    if extracted.has_sql {
        ogsql_complexity::analyze(&extracted.sql_text).ok()
    } else {
        None
    }
}

fn output_block(
    plan: &ogexplain_core::model::ExplainPlan,
    output: &str,
    threshold: &str,
    quiet: bool,
    complexity: Option<&ogsql_complexity::ComplexityReport>,
    num: usize,
    total: usize,
) -> Result<()> {
    if total > 1 {
        println!();
        println!(
            "{}",
            format!("═══ Block {}/{} ═══", num, total)
                .bright_cyan()
                .bold()
        );
    }
    analyze_and_output(plan, output, threshold, quiet, complexity)
}

fn output_sql_only(sql: &str, output: &str, num: usize, total: usize) -> Result<()> {
    let report = match ogsql_complexity::analyze(sql) {
        Ok(r) => r,
        Err(_) => return Ok(()),
    };

    if total > 1 {
        println!();
        println!(
            "{}",
            format!("═══ Block {}/{} (SQL only) ═══", num, total)
                .bright_cyan()
                .bold()
        );
    }

    match output {
        "json" => {
            #[derive(serde::Serialize)]
            struct SqlOnly<'a> {
                complexity: &'a ogsql_complexity::ComplexityReport,
                findings: [(); 0],
                suggestions: [(); 0],
            }
            let out = SqlOnly {
                complexity: &report,
                findings: [],
                suggestions: [],
            };
            println!("{}", serde_json::to_string_pretty(&out)?);
        }
        _ => {
            print_complexity_section(&report);
        }
    }
    Ok(())
}

fn analyze_and_output(
    plan: &ogexplain_core::model::ExplainPlan,
    output: &str,
    threshold: &str,
    quiet: bool,
    complexity: Option<&ogsql_complexity::ComplexityReport>,
) -> Result<()> {
    let diag_report = ogexplain_core::analyze(plan);
    let suggestions = SuggestionEngine::suggest(&diag_report.findings);

    let min_severity = parse_severity(threshold);
    let filtered_findings: Vec<_> = diag_report
        .findings
        .iter()
        .filter(|f| f.severity <= min_severity)
        .collect();

    match output {
        "json" => output_json(
            plan,
            &filtered_findings,
            &suggestions,
            &diag_report.stats,
            complexity,
        )?,
        _ => output_text(
            plan,
            &filtered_findings,
            &suggestions,
            &diag_report.stats,
            quiet,
            complexity,
        )?,
    }

    Ok(())
}

fn read_input(file: &str) -> Result<String> {
    if file == "-" {
        let mut buf = String::new();
        io::stdin()
            .read_to_string(&mut buf)
            .context("Failed to read from stdin")?;
        Ok(buf)
    } else {
        std::fs::read_to_string(file).context(format!("Failed to read file: {}", file))
    }
}

fn parse_severity(s: &str) -> ogexplain_core::analyzer::report::Severity {
    match s {
        "critical" => ogexplain_core::analyzer::report::Severity::Critical,
        "warning" => ogexplain_core::analyzer::report::Severity::Warning,
        _ => ogexplain_core::analyzer::report::Severity::Info,
    }
}

fn output_text(
    plan: &ogexplain_core::model::ExplainPlan,
    findings: &[&ogexplain_core::analyzer::report::Finding],
    suggestions: &[ogexplain_core::suggester::Suggestion],
    _stats: &ogexplain_core::analyzer::context::GlobalStats,
    quiet: bool,
    complexity: Option<&ogsql_complexity::ComplexityReport>,
) -> Result<()> {
    if !quiet {
        println!(
            "{}",
            "══════════════════════════════════════════════".bright_cyan()
        );
        println!(
            "{}",
            "  OpenGauss Execution Plan Analysis Report"
                .bright_cyan()
                .bold()
        );
        println!(
            "{}",
            "══════════════════════════════════════════════".bright_cyan()
        );
        println!();

        print_plan_tree(&plan.root, &plan.summary);

        if let Some(report) = complexity {
            println!();
            print_complexity_section(report);
        }

        println!();
    }

    let criticals: Vec<_> = findings
        .iter()
        .filter(|f| f.severity == ogexplain_core::analyzer::report::Severity::Critical)
        .collect();
    let warnings: Vec<_> = findings
        .iter()
        .filter(|f| f.severity == ogexplain_core::analyzer::report::Severity::Warning)
        .collect();
    let infos: Vec<_> = findings
        .iter()
        .filter(|f| f.severity == ogexplain_core::analyzer::report::Severity::Info)
        .collect();

    if !criticals.is_empty() {
        println!(
            "{} {}",
            "🔴".red(),
            format!("Critical ({})", criticals.len()).red().bold()
        );
        println!("{}", "──────────────────".red());
        for f in &criticals {
            print_finding(f);
        }
        println!();
    }

    if !warnings.is_empty() {
        println!(
            "{} {}",
            "🟡".yellow(),
            format!("Warnings ({})", warnings.len()).yellow().bold()
        );
        println!("{}", "───────────────".yellow());
        for f in &warnings {
            print_finding(f);
        }
        println!();
    }

    if !infos.is_empty() {
        println!(
            "{} {}",
            "🟢".green(),
            format!("Info ({})", infos.len()).green()
        );
        println!("{}", "──────────".green());
        for f in &infos {
            print_finding(f);
        }
        println!();
    }

    if findings.is_empty() && !quiet {
        println!("{}", "No issues found.".green());
    }

    if !suggestions.is_empty() {
        println!("💡 {}", "Suggestions".bright_white().bold());
        println!("{}", "──────────────".bright_white());
        for (i, s) in suggestions.iter().enumerate() {
            let confidence_label = if s.confidence >= 0.85 {
                "High"
            } else if s.confidence >= 0.7 {
                "Medium"
            } else {
                "Low"
            };
            println!(
                "  {}. [{}] {}",
                i + 1,
                confidence_label.bright_yellow(),
                s.message
            );
        }
    }

    Ok(())
}

fn print_finding(f: &ogexplain_core::analyzer::report::Finding) {
    println!(
        "  [{}] {}",
        f.rule_id.bright_white().bold(),
        f.title.bright_white()
    );
    if let Some(line) = f.node_line {
        let node_type = f.node_type.as_deref().unwrap_or("unknown");
        println!("    Node: \"{}\" (line {})", node_type, line);
    }
    println!("    {}", f.detail);
    if let Some(suggestion) = &f.suggestion {
        println!("    {}", format!("Suggestion: {}", suggestion).dimmed());
    }
}

fn print_plan_tree(
    node: &ogexplain_core::model::PlanNode,
    summary: &Option<ogexplain_core::model::PlanSummary>,
) {
    println!("{}", "Plan Tree".bright_cyan().bold());
    println!("{}", "─────────".bright_cyan());
    print_node(node, 0);
    if let Some(s) = summary {
        if let Some(rt) = s.total_runtime_ms {
            println!();
            println!("  Total runtime: {:.3} ms", rt);
        }
        if let Some(mem) = s.peak_memory_kb {
            println!("  Peak memory: {} kB", mem);
        }
    }
}

fn print_node(node: &ogexplain_core::model::PlanNode, depth: usize) {
    let indent = "  ".repeat(depth);

    let mut header = format!("{}{}", indent, node.node_type);
    if let Some(rel) = &node.relation {
        header.push_str(&format!(" on {}", rel));
    }

    let mut cost_parts: Vec<String> = Vec::new();
    if let Some(est) = &node.estimated {
        cost_parts.push(format!(
            "cost={:.2}..{:.2} rows={:.0} width={}",
            est.startup_cost, est.total_cost, est.plan_rows, est.plan_width
        ));
    }
    if let Some(act) = &node.actual {
        if act.executed {
            cost_parts.push(format!(
                "actual={:.3}..{:.3}ms rows={:.0} loops={:.0}",
                act.startup_time_ms, act.total_time_ms, act.rows, act.loops
            ));
        } else {
            cost_parts.push("(never executed)".to_string());
        }
    }

    if cost_parts.is_empty() {
        println!("{}", header);
    } else {
        println!("{} ({})", header, cost_parts.join("  ").dimmed());
    }

    for prop in &node.properties {
        println!(
            "{}{} {}: {}",
            indent,
            "  ".repeat(1),
            prop.label,
            prop.value.dimmed()
        );
    }

    if let Some(buffers) = &node.buffers {
        let mut buf_parts: Vec<String> = Vec::new();
        if buffers.shared_hit > 0 || buffers.shared_read > 0 {
            buf_parts.push(format!(
                "shared hit={} read={}",
                buffers.shared_hit, buffers.shared_read
            ));
        }
        if buffers.temp_read > 0 || buffers.temp_written > 0 {
            buf_parts.push(format!(
                "temp read={} written={}",
                buffers.temp_read, buffers.temp_written
            ));
        }
        if !buf_parts.is_empty() {
            println!("{}  Buffers: {}", indent, buf_parts.join(", ").dimmed());
        }
    }

    for child in &node.children {
        print_node(child, depth + 1);
    }
}

fn print_complexity_section(report: &ogsql_complexity::ComplexityReport) {
    use ogsql_complexity::ComplexityLevel;

    println!("{}", "SQL Complexity".bright_magenta().bold());

    let (level_color, level_icon) = match report.overall_level {
        ComplexityLevel::Trivial => ("green", "●"),
        ComplexityLevel::Simple => ("green", "◐"),
        ComplexityLevel::Moderate => ("yellow", "◑"),
        ComplexityLevel::Complex => ("red", "◉"),
        ComplexityLevel::VeryComplex => ("magenta", "✖"),
    };

    let score_str = format!("{:.1}", report.overall_score);
    let level_str = report.overall_level.label();
    let profile_str = format!("({})", report.profile);

    print!("  {} ", level_icon);
    print!("{}", score_str.bright_white().bold());
    print!(" ");
    match level_color {
        "green" => print!("{}", level_str.green().bold()),
        "yellow" => print!("{}", level_str.yellow().bold()),
        "red" => print!("{}", level_str.red().bold()),
        _ => print!("{}", level_str.bright_magenta().bold()),
    }
    println!(" {}", profile_str.dimmed());

    for (i, stmt) in report.statements.iter().enumerate() {
        if report.statements.len() > 1 {
            println!(
                "  {} {}",
                format!("[{}]", i + 1).bright_yellow(),
                format!("score {:.1}", stmt.adjusted_score).dimmed()
            );
        }

        let m = &stmt.metrics;
        let b = &stmt.weighted_breakdown;
        let mut parts: Vec<String> = Vec::new();

        if m.table_count > 0 {
            parts.push(format!("{}表(={:.1})", m.table_count, b.tables));
        }
        if m.join_count > 0 {
            parts.push(format!("{}连接(={:.1})", m.join_count, b.joins));
        }
        if m.where_condition_count > 0 {
            parts.push(format!(
                "{}条件(={:.1})",
                m.where_condition_count, b.where_conditions
            ));
        }
        if m.subquery_count > 0 {
            parts.push(format!("{}子查询(={:.1})", m.subquery_count, b.subqueries));
        }
        if m.aggregate_function_count > 0 {
            parts.push(format!(
                "{}聚合(={:.1})",
                m.aggregate_function_count, b.aggregate_functions
            ));
        }
        if m.case_expression_count > 0 {
            parts.push(format!(
                "{}CASE(={:.1})",
                m.case_expression_count, b.case_expressions
            ));
        }
        if m.set_operation_count > 0 {
            parts.push(format!(
                "{}集合(={:.1})",
                m.set_operation_count, b.set_operations
            ));
        }
        if m.has_group_by {
            parts.push(format!("GROUP BY({:.1})", b.group_by));
        }
        if m.has_order_by {
            parts.push(format!("ORDER BY({:.1})", b.order_by));
        }
        if m.has_distinct {
            parts.push("DISTINCT".to_string());
        }
        if m.window_function_count > 0 {
            parts.push(format!(
                "{}窗口(={:.1})",
                m.window_function_count, b.window_functions
            ));
        }
        if m.cte_count > 0 {
            parts.push(format!("{}CTE(={:.1})", m.cte_count, b.ctes));
        }

        if !parts.is_empty() {
            println!("    {}", parts.join("  "));
        }

        if m.subquery_depth > 0 {
            println!("    嵌套深度: {}", m.subquery_depth);
        }

        let sql_preview: String = stmt
            .sql_text
            .lines()
            .take(2)
            .map(|l| {
                if l.len() > 80 {
                    format!("{}...", &l[..80])
                } else {
                    l.to_string()
                }
            })
            .collect::<Vec<_>>()
            .join(" ");
        println!("    {}", sql_preview.dimmed());
    }
}

fn output_json(
    plan: &ogexplain_core::model::ExplainPlan,
    findings: &[&ogexplain_core::analyzer::report::Finding],
    suggestions: &[ogexplain_core::suggester::Suggestion],
    stats: &ogexplain_core::analyzer::context::GlobalStats,
    complexity: Option<&ogsql_complexity::ComplexityReport>,
) -> Result<()> {
    #[derive(serde::Serialize)]
    struct JsonOutput<'a> {
        plan: &'a ogexplain_core::model::ExplainPlan,
        complexity: Option<&'a ogsql_complexity::ComplexityReport>,
        findings: Vec<&'a ogexplain_core::analyzer::report::Finding>,
        suggestions: &'a [ogexplain_core::suggester::Suggestion],
        stats: &'a ogexplain_core::analyzer::context::GlobalStats,
    }
    let output = JsonOutput {
        plan,
        complexity,
        findings: findings.to_vec(),
        suggestions,
        stats,
    };
    let json = serde_json::to_string_pretty(&output)?;
    println!("{}", json);
    Ok(())
}
