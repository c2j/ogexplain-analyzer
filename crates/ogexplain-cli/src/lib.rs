rust_i18n::i18n!("../ogexplain-core/i18n", fallback = "en");

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use colored::*;
use ogexplain_core::i18n;
use ogexplain_core::suggester::SuggestionEngine;
use ogexplain_core::summary::{ComplexityInput, SummaryRow};
use rust_i18n::t;
use std::io::{self, Read};

#[cfg(feature = "db")]
pub mod db;

#[derive(Parser)]
#[command(name = "ogexplain")]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    Analyze {
        file: String,
        #[arg(short, long, default_value = "text")]
        output: String,
        #[arg(long, default_value = "info")]
        threshold: String,
        #[arg(short, long)]
        quiet: bool,
        #[arg(short, long)]
        verbose: bool,
        #[arg(long)]
        multi: bool,
        #[arg(long)]
        csv: Option<String>,
        #[arg(long, default_value = "auto")]
        lang: String,
    },
    Explain {
        /// Database connection string
        #[arg(short, long)]
        dsn: String,
        /// SQL statement (inline)
        #[arg(short = 's', long)]
        sql: Option<String>,
        /// SQL file path
        #[arg(short = 'f', long = "sql-file")]
        sql_file: Option<String>,
        /// Run EXPLAIN ANALYZE (actually executes the query)
        #[arg(long)]
        analyze: bool,
        /// Output format
        #[arg(short, long, default_value = "text")]
        output: String,
        /// Minimum severity threshold
        #[arg(long, default_value = "info")]
        threshold: String,
        /// Only show findings, no summary
        #[arg(short, long)]
        quiet: bool,
        /// Export summary to CSV
        #[arg(long)]
        csv: Option<String>,
        /// Language
        #[arg(long, default_value = "auto")]
        lang: String,
    },
}

fn fmt_cost(v: f64) -> String {
    if v >= 1_000_000.0 {
        format!("{:.1}M", v / 1_000_000.0)
    } else if v >= 1000.0 {
        format!("{:.1}K", v / 1000.0)
    } else {
        format!("{:.2}", v)
    }
}

fn fmt_time(v: f64) -> String {
    if v >= 1000.0 {
        format!("{:.2}s", v / 1000.0)
    } else {
        format!("{:.3}ms", v)
    }
}

fn fmt_rows(v: f64) -> String {
    if v >= 1_000_000.0 {
        format!("{:.1}M", v / 1_000_000.0)
    } else if v >= 1000.0 {
        format!("{:.1}K", v / 1000.0)
    } else {
        format!("{:.0}", v)
    }
}

fn fmt_kb(v: f64) -> String {
    if v >= 1024.0 {
        format!("{:.1}MB", v / 1024.0)
    } else {
        format!("{:.0}kB", v)
    }
}

fn to_complexity_input(
    report: &ogsql_complexity::ComplexityReport,
    gauss_report: Option<&ogsql_complexity::GaussDbComplexityReport>,
) -> ComplexityInput {
    let first = match report.statements.first() {
        Some(s) => s,
        None => {
            return ComplexityInput::default();
        }
    };
    let sql_preview = {
        let single_line: String = first.sql_text.lines().collect::<Vec<_>>().join(" ");
        let truncated = if single_line.chars().count() > 50 {
            format!("{}..", single_line.chars().take(48).collect::<String>())
        } else {
            single_line
        };
        Some(truncated)
    };
    ComplexityInput {
        sql_preview,
        tables: first.metrics.table_count,
        joins: first.metrics.join_count,
        subqueries: first.metrics.subquery_count,
        where_conditions: gauss_report
            .map(|g| g.pl_metrics.where_condition_count)
            .unwrap_or(0),
        aggregates: gauss_report
            .map(|g| g.pl_metrics.aggregate_function_count)
            .unwrap_or(0),
        cases: gauss_report
            .map(|g| g.pl_metrics.case_expression_count)
            .unwrap_or(0),
        set_ops: gauss_report
            .map(|g| g.pl_metrics.set_operation_count)
            .unwrap_or(0),
        ctes: gauss_report.map(|g| g.pl_metrics.cte_count).unwrap_or(0),
        windows: gauss_report
            .map(|g| g.pl_metrics.window_function_count)
            .unwrap_or(0),
        has_group_by: gauss_report
            .map(|g| g.pl_metrics.has_group_by)
            .unwrap_or(false),
        has_order_by: gauss_report
            .map(|g| g.pl_metrics.has_order_by)
            .unwrap_or(false),
        has_distinct: gauss_report
            .map(|g| g.pl_metrics.has_distinct)
            .unwrap_or(false),
        subquery_depth: gauss_report
            .map(|g| g.pl_metrics.subquery_depth)
            .unwrap_or(0),
        hints: gauss_report.map(|g| g.pl_metrics.hint_count).unwrap_or(0),
        score: Some(report.overall_score),
        level: Some(report.overall_level.label().to_string()),
        gauss_score: gauss_report.map(|g| g.overall_score),
        gauss_level: gauss_report.map(|g| g.level.label().to_string()),
        sql_category: gauss_report.map(|g| g.sql_category.label().to_string()),
        sql_sub_type: gauss_report.map(|g| g.sql_sub_type.clone()),
        gauss_sql_structure: gauss_report.map(|g| g.dimensions.sql_structure),
        gauss_pl_logic: gauss_report.map(|g| g.dimensions.pl_logic),
        gauss_advanced_feature: gauss_report.map(|g| g.dimensions.advanced_feature),
        gauss_extension: gauss_report.map(|g| g.dimensions.extension),
        gauss_tags: gauss_report
            .map(|g| g.tags.iter().map(|t| t.label().to_string()).collect())
            .unwrap_or_default(),
    }
}

fn print_summary_table(rows: &[SummaryRow]) {
    let separator = "═".repeat(207);
    println!();
    println!("{}", separator.bright_cyan().bold());
    println!("{}", t!("cli.summary.title").bright_cyan().bold());
    println!("{}", separator.bright_cyan().bold());

    println!(
        "{}",
        format!(
            "{:<3} {:<27} {:>4} {:>4} {:>4} {:>3} {:>3} {:>3} {:>3} {:>3} {:>3} {:>3} {:>3} {:>3} {:>3} {:>3} {:>6} {:<8} {:>6} {:<16} {:<12} {:>8} {:>10} {:>7} {:>6} {:>8} {:>8} {:>4} {:>6} {:>6} {:>7} {:>7} {:>5} {:>6} {:>7} {:>7} {}",
            t!("cli.summary.col_num"), t!("cli.summary.col_sql"), t!("cli.summary.col_tables"), t!("cli.summary.col_joins"), t!("cli.summary.col_subq"), t!("cli.summary.col_whr"), t!("cli.summary.col_agg"), t!("cli.summary.col_case"), t!("cli.summary.col_set"), t!("cli.summary.col_grp"), t!("cli.summary.col_ord"), t!("cli.summary.col_dst"), t!("cli.summary.col_hnt"), t!("cli.summary.col_cte"), t!("cli.summary.col_win"), t!("cli.summary.col_dep"), t!("cli.summary.col_score"), t!("cli.summary.col_level"), t!("cli.summary.col_gscore"), t!("cli.summary.col_type"), t!("cli.summary.col_op"), t!("cli.summary.col_cost"), t!("cli.summary.col_time"), t!("cli.summary.col_rows"), t!("cli.summary.col_estdelta"), t!("cli.summary.col_spill"), t!("cli.summary.col_mem"), t!("cli.summary.col_hitpct"), t!("cli.summary.col_tmpr"), t!("cli.summary.col_tmpw"), t!("cli.summary.col_fltr"), t!("cli.summary.col_erows"), t!("cli.summary.col_loops"), t!("cli.summary.col_net"), t!("cli.summary.col_ptime"), t!("cli.summary.col_cwi"), t!("cli.summary.col_tags")
        )
        .bright_cyan()
        .bold()
    );

    for (i, row) in rows.iter().enumerate() {
        let num = format!("{}", i + 1);
        let sql = match &row.sql_preview {
            Some(s) => {
                if s.chars().count() > 27 {
                    format!("{}..", s.chars().take(25).collect::<String>())
                } else {
                    s.clone()
                }
            }
            None => t!("cli.summary.no_sql").to_string(),
        };
        let tbl = format!("{}", row.tables);
        let join = format!("{}", row.joins);
        let subq = format!("{}", row.subqueries);
        let whr = if row.where_conditions > 0 {
            format!("{}", row.where_conditions)
        } else {
            "-".to_string()
        };
        let agg = if row.aggregates > 0 {
            format!("{}", row.aggregates)
        } else {
            "-".to_string()
        };
        let case = if row.cases > 0 {
            format!("{}", row.cases)
        } else {
            "-".to_string()
        };
        let set = if row.set_ops > 0 {
            format!("{}", row.set_ops)
        } else {
            "-".to_string()
        };
        let grp = if row.has_group_by {
            "Y".to_string()
        } else {
            "-".to_string()
        };
        let ord = if row.has_order_by {
            "Y".to_string()
        } else {
            "-".to_string()
        };
        let dst = if row.has_distinct {
            "Y".to_string()
        } else {
            "-".to_string()
        };
        let hnt = if row.hints > 0 {
            format!("{}", row.hints)
        } else {
            "-".to_string()
        };
        let cte = if row.ctes > 0 {
            format!("{}", row.ctes)
        } else {
            "-".to_string()
        };
        let win = if row.windows > 0 {
            format!("{}", row.windows)
        } else {
            "-".to_string()
        };
        let dep = if row.subquery_depth > 0 {
            format!("{}", row.subquery_depth)
        } else {
            "-".to_string()
        };
        let score = match row.score {
            Some(s) => format!("{:.1}", s),
            None => "-".to_string(),
        };
        let level_raw = row.level.as_deref().unwrap_or("-");
        let level_colored = match level_raw {
            "Trivial" | "Simple" => level_raw.green().to_string(),
            "Moderate" => level_raw.yellow().to_string(),
            "Complex" | "VeryComplex" => level_raw.red().to_string(),
            _ => level_raw.to_string(),
        };
        let gscore = match row.gauss_score {
            Some(s) => {
                let s_str = format!("{}", s);
                if s < 30 {
                    s_str.green().to_string()
                } else if s < 60 {
                    s_str.yellow().to_string()
                } else {
                    s_str.red().to_string()
                }
            }
            None => "-".to_string(),
        };
        let type_str = row.sql_sub_type.as_deref().unwrap_or("-");
        let type_colored = match row.sql_category.as_deref() {
            Some("Query") => type_str.green().to_string(),
            Some("DML") => type_str.yellow().to_string(),
            Some("DDL") => type_str.cyan().to_string(),
            Some("PL") => type_str.magenta().to_string(),
            Some("Pkg") => type_str.blue().to_string(),
            _ => type_str.to_string(),
        };
        let op = {
            let o = &row.root_op;
            if o.chars().count() > 12 {
                format!("{}..", o.chars().take(10).collect::<String>())
            } else {
                o.clone()
            }
        };
        let cost = fmt_cost(row.total_cost);
        let time = fmt_time(row.total_time_ms);
        let rows_str = match row.actual_rows {
            Some(r) => fmt_rows(r),
            None => "-".to_string(),
        };
        let est_delta = match row.worst_est_ratio {
            Some(r) => format!("{:.1}x", r),
            None => "-".to_string(),
        };
        let est_colored = match row.worst_est_ratio {
            Some(r) if r > 10.0 => est_delta.red().to_string(),
            Some(r) if r > 3.0 => est_delta.yellow().to_string(),
            _ => est_delta.clone(),
        };
        let spill = match row.spill_kb {
            Some(kb) if kb > 0.0 => fmt_kb(kb).yellow().to_string(),
            _ => "-".to_string(),
        };
        let mem = match row.peak_memory_kb {
            Some(kb) => fmt_kb(kb),
            None => "-".to_string(),
        };
        let hit_pct = match row.buffer_hit_rate {
            Some(rate) => {
                let s = format!("{:.0}%", rate);
                if rate >= 80.0 {
                    s.green().to_string()
                } else if rate >= 50.0 {
                    s.yellow().to_string()
                } else {
                    s.red().to_string()
                }
            }
            None => "-".to_string(),
        };
        let tmp_r = match row.total_temp_read_kb {
            Some(kb) if kb > 0.0 => fmt_kb(kb).yellow().to_string(),
            _ => "-".to_string(),
        };
        let tmp_w = match row.total_temp_written_kb {
            Some(kb) if kb > 0.0 => fmt_kb(kb).yellow().to_string(),
            _ => "-".to_string(),
        };
        let flt_r = match row.max_filter_removed {
            Some(r) => {
                let s = fmt_rows(r);
                if r > 1000.0 {
                    s.yellow().to_string()
                } else {
                    s
                }
            }
            None => "-".to_string(),
        };
        let e_rows = match row.estimated_rows {
            Some(r) => fmt_rows(r),
            None => "-".to_string(),
        };
        let loops = match row.total_loops {
            Some(l) => format!("{}", l as u64),
            None => "-".to_string(),
        };
        let net = match row.network_kb {
            Some(kb) => fmt_kb(kb),
            None => "-".to_string(),
        };
        let ptime = match row.planner_time_ms {
            Some(t) => fmt_time(t),
            None => "-".to_string(),
        };
        let cwi = format!(
            "{}/{}/{}",
            row.critical_count, row.warning_count, row.info_count
        );
        let cwi_colored = if row.critical_count > 0 {
            cwi.red().to_string()
        } else if row.warning_count > 0 {
            cwi.yellow().to_string()
        } else {
            cwi.green().to_string()
        };
        let tags = if row.gauss_tags.is_empty() {
            "-".to_string()
        } else {
            row.gauss_tags.join(",")
        };

        println!(
            "{:<3} {:<27} {:>4} {:>4} {:>4} {:>3} {:>3} {:>3} {:>3} {:>3} {:>3} {:>3} {:>3} {:>3} {:>3} {:>3} {:>6} {:<8} {:>6} {:<16} {:<12} {:>8} {:>10} {:>7} {:>6} {:>8} {:>8} {:>4} {:>6} {:>6} {:>7} {:>7} {:>5} {:>6} {:>7} {:>7} {}",
            num, sql, tbl, join, subq, whr, agg, case, set, grp, ord, dst, hnt, cte, win, dep, score, level_colored, gscore, type_colored, op, cost, time, rows_str, est_colored, spill, mem, hit_pct, tmp_r, tmp_w, flt_r, e_rows, loops, net, ptime, cwi_colored, tags
        );
    }

    println!("{}", separator.bright_cyan());
}

/// Escape a value for CSV output per RFC 4180.
/// If the value contains a comma, double quote, or newline, wrap in quotes
/// and double any internal quotes.
fn csv_escape(value: &str) -> String {
    if value.contains(',') || value.contains('"') || value.contains('\n') || value.contains('\r') {
        let escaped = value.replace('"', "\"\"");
        format!("\"{}\"", escaped)
    } else {
        value.to_string()
    }
}

fn fmt_csv_opt_f64(v: Option<f64>) -> String {
    match v {
        Some(n) => format!("{}", n),
        None => String::new(),
    }
}

fn fmt_csv_opt_i64(v: Option<i64>) -> String {
    match v {
        Some(n) => format!("{}", n),
        None => String::new(),
    }
}

fn fmt_csv_bool(v: bool) -> &'static str {
    if v {
        "Y"
    } else {
        "N"
    }
}

fn fmt_csv_pushdown(status: &ogexplain_core::summary::PushdownStatus) -> &'static str {
    match status {
        ogexplain_core::summary::PushdownStatus::Pushed => "Pushed",
        ogexplain_core::summary::PushdownStatus::NotPushed => "NotPushed",
        ogexplain_core::summary::PushdownStatus::Local => "Local",
    }
}

fn fmt_csv_opt_str(v: &Option<String>) -> String {
    match v {
        Some(s) => csv_escape(s),
        None => String::new(),
    }
}

/// Export summary rows to CSV. `output` is a file path or "-" for stdout.
fn export_csv(rows: &[SummaryRow], output: &str) -> Result<()> {
    let header = "sql_preview,sql_category,sql_sub_type,\
tables,joins,subqueries,where_conditions,aggregates,cases,set_ops,ctes,windows,\
has_group_by,has_order_by,has_distinct,subquery_depth,hints,\
score,level,gauss_score,gauss_level,\
gauss_sql_structure,gauss_pl_logic,gauss_advanced_feature,gauss_extension,\
gauss_tags,\
root_op,total_cost,total_time_ms,actual_rows,estimated_rows,\
plan_depth,node_count,total_loops,\
worst_est_ratio,spill_kb,peak_memory_kb,pushdown,\
buffer_hit_rate,total_temp_read_kb,total_temp_written_kb,max_filter_removed,\
network_kb,planner_time_ms,\
critical_count,warning_count,info_count";

    let mut lines: Vec<String> = vec![header.to_string()];

    for row in rows {
        let cols: Vec<String> = vec![
            fmt_csv_opt_str(&row.sql_preview),
            fmt_csv_opt_str(&row.sql_category),
            fmt_csv_opt_str(&row.sql_sub_type),
            format!("{}", row.tables),
            format!("{}", row.joins),
            format!("{}", row.subqueries),
            format!("{}", row.where_conditions),
            format!("{}", row.aggregates),
            format!("{}", row.cases),
            format!("{}", row.set_ops),
            format!("{}", row.ctes),
            format!("{}", row.windows),
            fmt_csv_bool(row.has_group_by).to_string(),
            fmt_csv_bool(row.has_order_by).to_string(),
            fmt_csv_bool(row.has_distinct).to_string(),
            format!("{}", row.subquery_depth),
            format!("{}", row.hints),
            fmt_csv_opt_f64(row.score),
            fmt_csv_opt_str(&row.level),
            fmt_csv_opt_i64(row.gauss_score),
            fmt_csv_opt_str(&row.gauss_level),
            fmt_csv_opt_i64(row.gauss_sql_structure),
            fmt_csv_opt_i64(row.gauss_pl_logic),
            fmt_csv_opt_i64(row.gauss_advanced_feature),
            fmt_csv_opt_i64(row.gauss_extension),
            csv_escape(&row.gauss_tags.join(";")),
            csv_escape(&row.root_op),
            format!("{}", row.total_cost),
            format!("{}", row.total_time_ms),
            fmt_csv_opt_f64(row.actual_rows),
            fmt_csv_opt_f64(row.estimated_rows),
            format!("{}", row.plan_depth),
            format!("{}", row.node_count),
            fmt_csv_opt_f64(row.total_loops),
            fmt_csv_opt_f64(row.worst_est_ratio),
            fmt_csv_opt_f64(row.spill_kb),
            fmt_csv_opt_f64(row.peak_memory_kb),
            fmt_csv_pushdown(&row.pushdown).to_string(),
            fmt_csv_opt_f64(row.buffer_hit_rate),
            fmt_csv_opt_f64(row.total_temp_read_kb),
            fmt_csv_opt_f64(row.total_temp_written_kb),
            fmt_csv_opt_f64(row.max_filter_removed),
            fmt_csv_opt_f64(row.network_kb),
            fmt_csv_opt_f64(row.planner_time_ms),
            format!("{}", row.critical_count),
            format!("{}", row.warning_count),
            format!("{}", row.info_count),
        ];
        lines.push(cols.join(","));
    }

    let csv_content = lines.join("\n");

    if output == "-" {
        println!("{}", csv_content);
    } else {
        std::fs::write(output, csv_content)
            .context(format!("Failed to write CSV to {}", output))?;
    }

    Ok(())
}

pub fn run() -> Result<()> {
    // Pre-scan --lang from raw args before clap builds help text
    let args_vec: Vec<String> = std::env::args().collect();
    let pre_lang = args_vec
        .windows(2)
        .find(|w| w[0] == "--lang")
        .map(|w| w[1].as_str())
        .unwrap_or("auto");
    let locale = if pre_lang == "auto" {
        i18n::detect_locale()
    } else {
        pre_lang.to_string()
    };
    i18n::init(Some(&locale));

    let cmd = clap::Command::new("ogexplain")
        .version(env!("CARGO_PKG_VERSION"))
        .about(t!("cli.about").to_string())
        .subcommand_required(false)
        .subcommand(
            clap::Command::new("analyze")
                .about(t!("cli.analyze.about").to_string())
                .arg(
                    clap::Arg::new("file")
                        .required(true)
                        .help(t!("cli.analyze.help_file").to_string()),
                )
                .arg(
                    clap::Arg::new("output")
                        .short('o')
                        .long("output")
                        .default_value("text")
                        .help(t!("cli.analyze.help_output").to_string()),
                )
                .arg(
                    clap::Arg::new("threshold")
                        .long("threshold")
                        .default_value("info")
                        .help(t!("cli.analyze.help_threshold").to_string()),
                )
                .arg(
                    clap::Arg::new("quiet")
                        .short('q')
                        .long("quiet")
                        .action(clap::ArgAction::SetTrue)
                        .help(t!("cli.analyze.help_quiet").to_string()),
                )
                .arg(
                    clap::Arg::new("verbose")
                        .short('v')
                        .long("verbose")
                        .action(clap::ArgAction::SetTrue)
                        .help(t!("cli.analyze.help_verbose").to_string()),
                )
                .arg(
                    clap::Arg::new("multi")
                        .long("multi")
                        .action(clap::ArgAction::SetTrue)
                        .help(t!("cli.analyze.help_multi").to_string()),
                )
                .arg(
                    clap::Arg::new("csv")
                        .long("csv")
                        .help(t!("cli.analyze.help_csv").to_string()),
                )
                .arg(
                    clap::Arg::new("lang")
                        .long("lang")
                        .default_value("auto")
                        .help(t!("cli.analyze.help_lang").to_string()),
                ),
        )
        .subcommand(
            clap::Command::new("explain")
                .about(t!("cli.explain.about").to_string())
                .arg(
                    clap::Arg::new("dsn")
                        .short('d')
                        .long("dsn")
                        .required(true)
                        .help(t!("cli.explain.help_dsn").to_string()),
                )
                .arg(
                    clap::Arg::new("sql")
                        .short('s')
                        .long("sql")
                        .help(t!("cli.explain.help_sql").to_string()),
                )
                .arg(
                    clap::Arg::new("sql_file")
                        .short('f')
                        .long("sql-file")
                        .help(t!("cli.explain.help_sql_file").to_string()),
                )
                .arg(
                    clap::Arg::new("analyze")
                        .long("analyze")
                        .action(clap::ArgAction::SetTrue)
                        .help(t!("cli.explain.help_analyze").to_string()),
                )
                .arg(
                    clap::Arg::new("output")
                        .short('o')
                        .long("output")
                        .default_value("text")
                        .help(t!("cli.explain.help_output").to_string()),
                )
                .arg(
                    clap::Arg::new("threshold")
                        .long("threshold")
                        .default_value("info")
                        .help(t!("cli.explain.help_threshold").to_string()),
                )
                .arg(
                    clap::Arg::new("quiet")
                        .short('q')
                        .long("quiet")
                        .action(clap::ArgAction::SetTrue)
                        .help(t!("cli.explain.help_quiet").to_string()),
                )
                .arg(
                    clap::Arg::new("csv")
                        .long("csv")
                        .help(t!("cli.explain.help_csv").to_string()),
                )
                .arg(
                    clap::Arg::new("lang")
                        .long("lang")
                        .default_value("auto")
                        .help(t!("cli.explain.help_lang").to_string()),
                ),
        )
        .subcommand(
            clap::Command::new("mcp")
                .about("Start MCP server (Model Context Protocol, stdio transport)"),
        );

    let matches = cmd.get_matches();

    match matches.subcommand() {
        Some(("explain", args)) => {
            #[cfg(feature = "db")]
            {
                let dsn = args.get_one::<String>("dsn").unwrap();
                let sql: Option<String> = args.get_one::<String>("sql").cloned();
                let sql_file: Option<String> = args.get_one::<String>("sql_file").cloned();
                let analyze = args.get_flag("analyze");
                let output = args
                    .get_one::<String>("output")
                    .map(|s| s.as_str())
                    .unwrap_or("text");
                let threshold = args
                    .get_one::<String>("threshold")
                    .map(|s| s.as_str())
                    .unwrap_or("info");
                let quiet = args.get_flag("quiet");
                let csv: Option<String> = args.get_one::<String>("csv").cloned();

                return run_explain(
                    dsn,
                    sql.as_deref(),
                    sql_file.as_deref(),
                    analyze,
                    output,
                    threshold,
                    quiet,
                    csv.as_deref(),
                );
            }
            #[cfg(not(feature = "db"))]
            {
                let _ = args;
                anyhow::bail!("Database support not compiled. Rebuild with --features db");
            }
        }
        Some(("mcp", _)) => {
            #[cfg(feature = "mcp")]
            {
                ogexplain_mcp::server::run();
                return Ok(());
            }
            #[cfg(not(feature = "mcp"))]
            {
                anyhow::bail!("MCP support not compiled. Rebuild with --features mcp");
            }
        }
        _ => {
            let (sub_name, args) = match matches.subcommand() {
                Some((name, args)) => (name, args),
                None => ("analyze", &matches),
            };
            let _ = sub_name;

            let file = args
                .get_one::<String>("file")
                .map(|s: &String| s.as_str())
                .unwrap_or("-");
            let output = args
                .get_one::<String>("output")
                .map(|s: &String| s.as_str())
                .unwrap_or("text");
            let threshold = args
                .get_one::<String>("threshold")
                .map(|s: &String| s.as_str())
                .unwrap_or("info");
            let quiet = args.get_flag("quiet");
            let _verbose = args.get_flag("verbose");
            let _multi = args.get_flag("multi");
            let csv: Option<String> = args.get_one::<String>("csv").cloned();

            {
                let input = read_input(file)?;

                let blocks = ogexplain_core::sql::segment_input(&input);
                let mut summary_rows: Vec<SummaryRow> = Vec::new();

                if blocks.is_empty() {
                    let plan = ogexplain_core::parse(&input)
                        .context(t!("cli.error.parse_failed").to_string())?;
                    let complexity = try_complexity(&input);
                    let gauss_complexity = try_gauss_complexity(&input);
                    let complexity_input = complexity
                        .as_ref()
                        .map(|r| to_complexity_input(r, gauss_complexity.as_ref()));
                    let diag = ogexplain_core::analyze(&plan);
                    let row = SummaryRow::compute(&plan, &diag, complexity_input.as_ref());
                    output_block_with_diag(
                        &plan,
                        &diag,
                        output,
                        threshold,
                        quiet,
                        complexity.as_ref(),
                        gauss_complexity.as_ref(),
                        1,
                        1,
                        Some(&row),
                    )?;
                    summary_rows.push(row);
                } else if blocks.len() == 1 {
                    let block = &blocks[0];
                    let plan = ogexplain_core::parse(&block.explain_text)
                        .context(t!("cli.error.parse_failed").to_string())?;
                    let complexity = block
                        .sql_text
                        .as_ref()
                        .and_then(|sql| ogsql_complexity::analyze(sql).ok());
                    let gauss_complexity = block.sql_text.as_ref().and_then(|sql| {
                        ogsql_complexity::gauss_analyze(
                            sql,
                            &ogsql_complexity::ComplexityConfig::default(),
                        )
                        .ok()
                    });
                    let complexity_input = complexity
                        .as_ref()
                        .map(|r| to_complexity_input(r, gauss_complexity.as_ref()));
                    let diag =
                        ogexplain_core::analyze_with_rewrite(&plan, block.sql_text.as_deref());
                    let row = SummaryRow::compute(&plan, &diag, complexity_input.as_ref());
                    output_block_with_diag(
                        &plan,
                        &diag,
                        output,
                        threshold,
                        quiet,
                        complexity.as_ref(),
                        gauss_complexity.as_ref(),
                        1,
                        1,
                        Some(&row),
                    )?;
                    summary_rows.push(row);
                } else {
                    for (i, block) in blocks.iter().enumerate() {
                        let num = i + 1;
                        let total = blocks.len();
                        if let Ok(plan) = ogexplain_core::parse(&block.explain_text) {
                            let complexity = block
                                .sql_text
                                .as_ref()
                                .and_then(|sql| ogsql_complexity::analyze(sql).ok());
                            let gauss_complexity = block.sql_text.as_ref().and_then(|sql| {
                                ogsql_complexity::gauss_analyze(
                                    sql,
                                    &ogsql_complexity::ComplexityConfig::default(),
                                )
                                .ok()
                            });
                            let complexity_input = complexity
                                .as_ref()
                                .map(|r| to_complexity_input(r, gauss_complexity.as_ref()));
                            let diag = ogexplain_core::analyze_with_rewrite(
                                &plan,
                                block.sql_text.as_deref(),
                            );
                            let row = SummaryRow::compute(&plan, &diag, complexity_input.as_ref());
                            output_block_with_diag(
                                &plan,
                                &diag,
                                output,
                                threshold,
                                quiet,
                                complexity.as_ref(),
                                gauss_complexity.as_ref(),
                                num,
                                total,
                                Some(&row),
                            )?;
                            summary_rows.push(row);
                        } else if let Some(sql) = &block.sql_text {
                            output_sql_only(sql, output, num, total)?;
                        }
                    }
                }

                if let Some(ref csv_path) = csv {
                    export_csv(&summary_rows, csv_path)?;
                }

                if output != "json" && !summary_rows.is_empty() {
                    print_summary_table(&summary_rows);
                }
            }
        }
    }

    Ok(())
}

#[cfg(feature = "db")]
#[allow(clippy::too_many_arguments)]
fn run_explain(
    dsn: &str,
    sql: Option<&str>,
    sql_file: Option<&str>,
    analyze: bool,
    output: &str,
    threshold: &str,
    quiet: bool,
    csv: Option<&str>,
) -> Result<()> {
    use crate::db;

    let sql_text = match (sql, sql_file) {
        (Some(s), None) => s.to_string(),
        (None, Some(path)) => std::fs::read_to_string(path)
            .context(t!("cli.explain.error.read_file", path = path).to_string())?,
        (Some(_), Some(_)) => {
            anyhow::bail!("Cannot use both -s and -f. Choose one.");
        }
        (None, None) => {
            anyhow::bail!("{}", t!("cli.explain.error.no_sql"));
        }
    };

    if analyze {
        eprintln!("{}", t!("cli.explain.warning_analyze").to_string().yellow());
    }

    let explain_text = db::fetch_explain(dsn, &sql_text, analyze)?;
    let plan =
        ogexplain_core::parse(&explain_text).context(t!("cli.error.parse_failed").to_string())?;
    let complexity = try_complexity(&sql_text);
    let gauss_complexity = try_gauss_complexity(&sql_text);
    let complexity_input = complexity
        .as_ref()
        .map(|r| to_complexity_input(r, gauss_complexity.as_ref()));
    let diag = ogexplain_core::analyze_with_rewrite(&plan, Some(&sql_text));
    let row = SummaryRow::compute(&plan, &diag, complexity_input.as_ref());

    output_block_with_diag(
        &plan,
        &diag,
        output,
        threshold,
        quiet,
        complexity.as_ref(),
        gauss_complexity.as_ref(),
        1,
        1,
        Some(&row),
    )?;

    if let Some(csv_path) = csv {
        export_csv(
            &[SummaryRow::compute(&plan, &diag, complexity_input.as_ref())],
            csv_path,
        )?;
    }

    if output != "json" {
        print_summary_table(&[SummaryRow::compute(&plan, &diag, complexity_input.as_ref())]);
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

fn try_gauss_complexity(input: &str) -> Option<ogsql_complexity::GaussDbComplexityReport> {
    let extracted = ogexplain_core::sql::ExtractedContent::from_text(input);
    if extracted.has_sql {
        ogsql_complexity::gauss_analyze(
            &extracted.sql_text,
            &ogsql_complexity::ComplexityConfig::default(),
        )
        .ok()
    } else {
        None
    }
}

#[allow(clippy::too_many_arguments)]
fn output_block_with_diag(
    plan: &ogexplain_core::model::ExplainPlan,
    diag_report: &ogexplain_core::analyzer::report::DiagnosticReport,
    output: &str,
    threshold: &str,
    quiet: bool,
    complexity: Option<&ogsql_complexity::ComplexityReport>,
    gauss_complexity: Option<&ogsql_complexity::GaussDbComplexityReport>,
    num: usize,
    total: usize,
    summary_row: Option<&SummaryRow>,
) -> Result<()> {
    if total > 1 {
        println!();
        println!(
            "{}",
            t!("cli.report.block_header", current = num, total = total)
                .bright_cyan()
                .bold()
        );
    }
    analyze_and_output(
        plan,
        output,
        threshold,
        quiet,
        complexity,
        gauss_complexity,
        diag_report,
        summary_row,
    )
}

fn output_sql_only(sql: &str, output: &str, num: usize, total: usize) -> Result<()> {
    let report = match ogsql_complexity::analyze(sql) {
        Ok(r) => r,
        Err(_) => return Ok(()),
    };
    let gauss_report =
        ogsql_complexity::gauss_analyze(sql, &ogsql_complexity::ComplexityConfig::default()).ok();

    if total > 1 {
        println!();
        println!(
            "{}",
            t!(
                "cli.report.block_header_sql_only",
                current = num,
                total = total
            )
            .bright_cyan()
            .bold()
        );
    }

    match output {
        "json" => {
            #[derive(serde::Serialize)]
            struct SqlOnly<'a> {
                complexity: &'a ogsql_complexity::ComplexityReport,
                gauss_complexity: Option<&'a ogsql_complexity::GaussDbComplexityReport>,
                findings: [(); 0],
                suggestions: [(); 0],
            }
            let out = SqlOnly {
                complexity: &report,
                gauss_complexity: gauss_report.as_ref(),
                findings: [],
                suggestions: [],
            };
            println!("{}", serde_json::to_string_pretty(&out)?);
        }
        _ => {
            print_complexity_section(&report, gauss_report.as_ref());
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn analyze_and_output(
    plan: &ogexplain_core::model::ExplainPlan,
    output: &str,
    threshold: &str,
    quiet: bool,
    complexity: Option<&ogsql_complexity::ComplexityReport>,
    gauss_complexity: Option<&ogsql_complexity::GaussDbComplexityReport>,
    diag_report: &ogexplain_core::analyzer::report::DiagnosticReport,
    summary_row: Option<&SummaryRow>,
) -> Result<()> {
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
            gauss_complexity,
            summary_row,
        )?,
        _ => output_text(
            plan,
            &filtered_findings,
            &suggestions,
            &diag_report.stats,
            quiet,
            complexity,
            gauss_complexity,
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
    gauss_complexity: Option<&ogsql_complexity::GaussDbComplexityReport>,
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
            print_complexity_section(report, gauss_complexity);
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
            t!("cli.findings.info", count = infos.len()).green()
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
    if let Some(rewrite) = &f.sql_rewrite {
        println!("    {}", "SQL Rewrite:".bright_green());
        println!("    {}", rewrite.rewritten_sql.bright_green());
        println!("    {}", rewrite.explanation.dimmed());
    }
}

fn print_plan_tree(
    node: &ogexplain_core::model::PlanNode,
    summary: &Option<ogexplain_core::model::PlanSummary>,
) {
    println!("{}", t!("cli.tree.header").bright_cyan().bold());
    println!("{}", "─────────".bright_cyan());
    print_node(node, 0);
    if let Some(s) = summary {
        if let Some(rt) = s.total_runtime_ms {
            println!();
            println!(
                "{}",
                t!("cli.tree.total_runtime", time = format!("{:.3}", rt))
            );
        }
        if let Some(mem) = s.peak_memory_kb {
            println!("{}", t!("cli.tree.peak_memory", mem = mem));
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
        println!("{}   {}: {}", indent, prop.label, prop.value.dimmed());
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

fn print_complexity_section(
    report: &ogsql_complexity::ComplexityReport,
    gauss: Option<&ogsql_complexity::GaussDbComplexityReport>,
) {
    use ogsql_complexity::model::gauss_weights;
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

    // GaussDB complexity section
    if let Some(g) = gauss {
        let gauss_score_str = format!("{}", g.overall_score);
        let gauss_level_str = g.level.label();
        let gauss_level_colored = match g.level {
            ComplexityLevel::Trivial | ComplexityLevel::Simple => {
                gauss_level_str.green().to_string()
            }
            ComplexityLevel::Moderate => gauss_level_str.yellow().to_string(),
            ComplexityLevel::Complex | ComplexityLevel::VeryComplex => {
                gauss_level_str.red().to_string()
            }
        };
        print!("{}", t!("cli.complexity.gauss_score"));
        print!("{}", gauss_score_str.bright_white().bold());
        print!(" ");
        println!("{}", gauss_level_colored);

        println!(
            "{}",
            t!(
                "cli.complexity.type",
                subtype = g.sql_sub_type,
                category = g.sql_category.label()
            )
        );

        let d = &g.dimensions;
        let mut dim_parts: Vec<String> = Vec::new();
        if d.sql_structure > 0 {
            dim_parts
                .push(t!("cli.complexity.dim_sql_structure", val = d.sql_structure).to_string());
        }
        if d.pl_logic > 0 {
            dim_parts.push(t!("cli.complexity.dim_pl_logic", val = d.pl_logic).to_string());
        }
        if d.advanced_feature > 0 {
            dim_parts.push(t!("cli.complexity.dim_advanced", val = d.advanced_feature).to_string());
        }
        if d.extension > 0 {
            dim_parts.push(t!("cli.complexity.dim_extension", val = d.extension).to_string());
        }
        if !dim_parts.is_empty() {
            println!(
                "{}",
                t!("cli.complexity.dimensions", dims = dim_parts.join(" "))
            );
        }

        if !g.tags.is_empty() {
            let tag_labels: Vec<String> = g.tags.iter().map(|t| t.label().to_string()).collect();
            println!(
                "{}",
                t!("cli.complexity.tags", tags = tag_labels.join(", "))
            );
        }

        let m = &g.pl_metrics;
        let mut has_pl_metrics = false;

        let mut sql_parts: Vec<String> = Vec::new();
        if m.table_count > 0 {
            sql_parts.push(
                t!(
                    "cli.complexity.tables",
                    count = m.table_count,
                    score = m.table_count as i64 * gauss_weights::TABLE
                )
                .to_string(),
            );
            has_pl_metrics = true;
        }
        if m.join_count > 0 {
            sql_parts.push(
                t!(
                    "cli.complexity.joins",
                    count = m.join_count,
                    score = m.join_count as i64 * gauss_weights::JOIN
                )
                .to_string(),
            );
            has_pl_metrics = true;
        }
        if m.where_condition_count > 0 {
            sql_parts.push(
                t!(
                    "cli.complexity.where",
                    count = m.where_condition_count,
                    score = m.where_condition_count as i64 * gauss_weights::WHERE_CONDITION
                )
                .to_string(),
            );
            has_pl_metrics = true;
        }
        if m.subquery_count > 0 {
            sql_parts.push(
                t!(
                    "cli.complexity.subqueries",
                    count = m.subquery_count,
                    score = m.subquery_count as i64 * gauss_weights::SUBQUERY
                )
                .to_string(),
            );
            has_pl_metrics = true;
        }
        if m.aggregate_function_count > 0 {
            sql_parts.push(
                t!(
                    "cli.complexity.aggregates",
                    count = m.aggregate_function_count,
                    score = m.aggregate_function_count as i64 * gauss_weights::AGGREGATE_FUNCTION
                )
                .to_string(),
            );
            has_pl_metrics = true;
        }
        if m.case_expression_count > 0 {
            sql_parts.push(
                t!(
                    "cli.complexity.cases",
                    count = m.case_expression_count,
                    score = m.case_expression_count as i64 * gauss_weights::CASE_EXPRESSION
                )
                .to_string(),
            );
            has_pl_metrics = true;
        }
        if m.has_group_by {
            sql_parts
                .push(t!("cli.complexity.group_by", score = gauss_weights::GROUP_BY).to_string());
            has_pl_metrics = true;
        }
        if m.has_order_by {
            sql_parts
                .push(t!("cli.complexity.order_by", score = gauss_weights::ORDER_BY).to_string());
            has_pl_metrics = true;
        }
        if m.hint_count > 0 {
            sql_parts.push(
                t!(
                    "cli.complexity.hints",
                    count = m.hint_count,
                    score = m.hint_count as i64 * gauss_weights::HINT
                )
                .to_string(),
            );
            has_pl_metrics = true;
        }
        if m.set_operation_count > 0 {
            sql_parts.push(
                t!(
                    "cli.complexity.set_ops",
                    count = m.set_operation_count,
                    score = m.set_operation_count as i64 * gauss_weights::SET_OPERATION
                )
                .to_string(),
            );
            has_pl_metrics = true;
        }
        if m.cte_count > 0 {
            sql_parts.push(t!("cli.complexity.ctes", count = m.cte_count).to_string());
            has_pl_metrics = true;
        }
        if m.window_function_count > 0 {
            sql_parts
                .push(t!("cli.complexity.windows", count = m.window_function_count).to_string());
            has_pl_metrics = true;
        }
        if m.has_distinct {
            sql_parts.push(t!("cli.complexity.distinct").to_string());
            has_pl_metrics = true;
        }
        if m.subquery_depth > 0 {
            sql_parts
                .push(t!("cli.complexity.nesting_depth", depth = m.subquery_depth).to_string());
            has_pl_metrics = true;
        }

        if has_pl_metrics
            || m.loop_count > 0
            || m.cursor_count > 0
            || m.dynamic_sql_count > 0
            || m.transaction_control_count > 0
        {
            println!("  {}", t!("cli.complexity.gauss_metrics").bright_magenta());
            if !sql_parts.is_empty() {
                println!("    {}", sql_parts.join("  "));
            }

            if m.loop_count > 0 {
                let loop_score = g.score_breakdown.loop_complexity;
                println!(
                    "{}",
                    t!(
                        "cli.complexity.loops",
                        count = m.loop_count,
                        depth = m.max_loop_nesting_level,
                        score = loop_score
                    )
                );
            }
            if m.cursor_count > 0 {
                let cursor_score = g.score_breakdown.cursor_complexity;
                println!(
                    "{}",
                    t!(
                        "cli.complexity.cursors",
                        count = m.cursor_count,
                        ops = m.cursor_operation_count,
                        score = cursor_score
                    )
                );
            }
            if m.dynamic_sql_count > 0 {
                println!(
                    "{}",
                    t!(
                        "cli.complexity.dynamic_sql",
                        count = m.dynamic_sql_count,
                        params = m.param_binding_count
                    )
                );
            }
            if m.transaction_control_count > 0 {
                let autonomous_label = if m.uses_autonomous_transactions {
                    t!("cli.complexity.yes")
                } else {
                    t!("cli.complexity.no")
                };
                println!(
                    "{}",
                    t!(
                        "cli.complexity.tx_control",
                        count = m.transaction_control_count,
                        auto = autonomous_label
                    )
                );
            }
        }
    }

    for (i, stmt) in report.statements.iter().enumerate() {
        if report.statements.len() > 1 {
            println!(
                "  {} {}",
                format!("[{}]", i + 1).bright_yellow(),
                t!(
                    "cli.complexity.score",
                    score = format!("{:.1}", stmt.adjusted_score)
                )
                .dimmed()
            );
        }

        let m = &stmt.metrics;
        let b = &stmt.weighted_breakdown;
        let mut parts: Vec<String> = Vec::new();

        if m.table_count > 0 {
            parts.push(
                t!(
                    "cli.complexity_stmt.tables",
                    count = m.table_count,
                    score = format!("{:.1}", b.tables)
                )
                .to_string(),
            );
        }
        if m.join_count > 0 {
            parts.push(
                t!(
                    "cli.complexity_stmt.joins",
                    count = m.join_count,
                    score = format!("{:.1}", b.joins)
                )
                .to_string(),
            );
        }
        if m.where_condition_count > 0 {
            parts.push(
                t!(
                    "cli.complexity_stmt.conditions",
                    count = m.where_condition_count,
                    score = format!("{:.1}", b.where_conditions)
                )
                .to_string(),
            );
        }
        if m.subquery_count > 0 {
            parts.push(
                t!(
                    "cli.complexity_stmt.subqueries",
                    count = m.subquery_count,
                    score = format!("{:.1}", b.subqueries)
                )
                .to_string(),
            );
        }
        if m.aggregate_function_count > 0 {
            parts.push(
                t!(
                    "cli.complexity_stmt.aggregates",
                    count = m.aggregate_function_count,
                    score = format!("{:.1}", b.aggregate_functions)
                )
                .to_string(),
            );
        }
        if m.case_expression_count > 0 {
            parts.push(
                t!(
                    "cli.complexity_stmt.cases",
                    count = m.case_expression_count,
                    score = format!("{:.1}", b.case_expressions)
                )
                .to_string(),
            );
        }
        if m.set_operation_count > 0 {
            parts.push(
                t!(
                    "cli.complexity_stmt.set_ops",
                    count = m.set_operation_count,
                    score = format!("{:.1}", b.set_operations)
                )
                .to_string(),
            );
        }
        if m.has_group_by {
            parts.push(
                t!(
                    "cli.complexity_stmt.group_by",
                    score = format!("{:.1}", b.group_by)
                )
                .to_string(),
            );
        }
        if m.has_order_by {
            parts.push(
                t!(
                    "cli.complexity_stmt.order_by",
                    score = format!("{:.1}", b.order_by)
                )
                .to_string(),
            );
        }
        if m.has_distinct {
            parts.push(t!("cli.complexity.distinct").to_string());
        }
        if m.window_function_count > 0 {
            parts.push(
                t!(
                    "cli.complexity_stmt.windows",
                    count = m.window_function_count,
                    score = format!("{:.1}", b.window_functions)
                )
                .to_string(),
            );
        }
        if m.cte_count > 0 {
            parts.push(
                t!(
                    "cli.complexity_stmt.ctes",
                    count = m.cte_count,
                    score = format!("{:.1}", b.ctes)
                )
                .to_string(),
            );
        }

        if !parts.is_empty() {
            println!("    {}", parts.join("  "));
        }

        if m.subquery_depth > 0 {
            println!(
                "{}",
                t!(
                    "cli.complexity_stmt.nesting_depth",
                    depth = m.subquery_depth
                )
            );
        }

        let sql_preview: String = stmt
            .sql_text
            .lines()
            .take(2)
            .map(|l| {
                if l.chars().count() > 80 {
                    format!("{}...", l.chars().take(80).collect::<String>())
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
    gauss_complexity: Option<&ogsql_complexity::GaussDbComplexityReport>,
    summary_row: Option<&SummaryRow>,
) -> Result<()> {
    #[derive(serde::Serialize)]
    struct JsonOutput<'a> {
        plan: &'a ogexplain_core::model::ExplainPlan,
        complexity: Option<&'a ogsql_complexity::ComplexityReport>,
        gauss_complexity: Option<&'a ogsql_complexity::GaussDbComplexityReport>,
        findings: Vec<&'a ogexplain_core::analyzer::report::Finding>,
        suggestions: &'a [ogexplain_core::suggester::Suggestion],
        stats: &'a ogexplain_core::analyzer::context::GlobalStats,
        summary: Option<&'a SummaryRow>,
    }
    let output = JsonOutput {
        plan,
        complexity,
        gauss_complexity,
        findings: findings.to_vec(),
        suggestions,
        stats,
        summary: summary_row,
    };
    let json = serde_json::to_string_pretty(&output)?;
    println!("{}", json);
    Ok(())
}
