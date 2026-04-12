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

fn main() -> Result<()> {
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
            multi,
        } => {
            let input = read_input(&file)?;

            if multi {
                let plans = ogexplain_core::parse_multi(&input)
                    .context("Failed to parse EXPLAIN output")?;
                for (i, plan) in plans.iter().enumerate() {
                    if plans.len() > 1 {
                        println!(
                            "\n{}",
                            format!("═══ EXPLAIN Block {}/{} ═══", i + 1, plans.len())
                                .bright_cyan()
                                .bold()
                        );
                    }
                    analyze_and_output(plan, &output, &threshold, quiet)?;
                }
            } else {
                let plan =
                    ogexplain_core::parse(&input).context("Failed to parse EXPLAIN output")?;
                analyze_and_output(&plan, &output, &threshold, quiet)?;
            }
        }
    }

    Ok(())
}

fn analyze_and_output(
    plan: &ogexplain_core::model::ExplainPlan,
    output: &str,
    threshold: &str,
    quiet: bool,
) -> Result<()> {
    let report = ogexplain_core::analyze(plan);
    let suggestions = SuggestionEngine::suggest(&report.findings);

    let min_severity = parse_severity(threshold);
    let filtered_findings: Vec<_> = report
        .findings
        .iter()
        .filter(|f| f.severity <= min_severity)
        .collect();

    match output {
        "json" => output_json(&filtered_findings, &suggestions, &report.stats)?,
        _ => output_text(&filtered_findings, &suggestions, &report.stats, quiet)?,
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
    findings: &[&ogexplain_core::analyzer::report::Finding],
    suggestions: &[ogexplain_core::suggester::Suggestion],
    stats: &ogexplain_core::analyzer::context::GlobalStats,
    quiet: bool,
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
        println!("Plan Overview");
        println!(
            "  Nodes: {}  Depth: {}  Max node time: {:.3} ms",
            stats.total_nodes, stats.max_depth, stats.max_node_time_ms
        );
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

fn output_json(
    findings: &[&ogexplain_core::analyzer::report::Finding],
    suggestions: &[ogexplain_core::suggester::Suggestion],
    stats: &ogexplain_core::analyzer::context::GlobalStats,
) -> Result<()> {
    #[derive(serde::Serialize)]
    struct JsonOutput<'a> {
        findings: Vec<&'a ogexplain_core::analyzer::report::Finding>,
        suggestions: &'a [ogexplain_core::suggester::Suggestion],
        stats: &'a ogexplain_core::analyzer::context::GlobalStats,
    }
    let output = JsonOutput {
        findings: findings.to_vec(),
        suggestions,
        stats,
    };
    let json = serde_json::to_string_pretty(&output)?;
    println!("{}", json);
    Ok(())
}
