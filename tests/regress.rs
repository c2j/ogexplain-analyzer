//! Per-rule regression driver (static mode).
//!
//! Walks `tests/regress/**/case.toml`, loads each case + its
//! `expected.findings.json`, reads the ogagila EXPLAIN material referenced by
//! the case, runs `parse()` + `analyze()`, and asserts findings match the
//! hand-authored contract.
//!
//! Per-case `#[test]` functions are generated at build time by `build.rs`
//! (into `OUT_DIR/regress_generated_tests.rs`); adding a new directory under
//! `tests/regress/<category>/` is sufficient — no manual registration.
//!
//! Live-DB mode (`--features live-db`, planned) will replay the SQL against a
//! fresh OpenGauss container instead of using pre-recorded EXPLAIN material.

use std::fs;
use std::path::{Path, PathBuf};

use ogexplain_core::analyzer::config::DiagnosticConfig;
use ogexplain_core::model::ExplainPlan;
use ogexplain_core::{analyze_with_config, parse};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct CaseManifest {
    rule_id: String,
    case_name: String,
    #[allow(dead_code)]
    expect_fired: bool,
    #[allow(dead_code)]
    expect_min_severity: String,
    dataset: DatasetConfig,
    #[serde(default)]
    #[allow(dead_code)]
    side_effects: SideEffectsConfig,
    #[serde(default)]
    #[allow(dead_code)]
    verification: VerificationConfig,
    #[serde(default)]
    config: Option<DiagnosticConfig>,
}

#[derive(Debug, Deserialize)]
struct DatasetConfig {
    source: String,
    ogagila_version: Option<String>,
    ogagila_query_ids: Option<Vec<String>>,
    supplemental_file: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct SideEffectsConfig {
    #[serde(default)]
    #[allow(dead_code)]
    requires_set: std::collections::HashMap<String, String>,
    #[serde(default)]
    #[allow(dead_code)]
    modifies_data: bool,
    #[serde(default)]
    #[allow(dead_code)]
    requires_delete_stats: bool,
}

#[derive(Debug, Default, Deserialize)]
struct VerificationConfig {
    #[serde(default)]
    #[allow(dead_code)]
    live_db_verify: bool,
    #[serde(default)]
    #[allow(dead_code)]
    weak_signal: bool,
    #[serde(default)]
    #[allow(dead_code)]
    skip_live_reason: String,
}

#[derive(Debug, Deserialize)]
struct ExpectedFile {
    #[allow(dead_code)]
    _meta: ExpectedMeta,
    findings: Vec<ExpectedFinding>,
    #[serde(default)]
    anti_findings: Vec<AntiFinding>,
}

#[derive(Debug, Deserialize)]
struct ExpectedMeta {
    #[allow(dead_code)]
    ogagila_commit: String,
    #[allow(dead_code)]
    ogagila_version: String,
    #[allow(dead_code)]
    ogagila_query_ids: Vec<String>,
    #[allow(dead_code)]
    ogexplain_version: String,
    #[allow(dead_code)]
    authored_at: String,
    #[allow(dead_code)]
    author: String,
    #[allow(dead_code)]
    review_notes: String,
}

#[derive(Debug, Deserialize)]
struct ExpectedFinding {
    rule_id: String,
    must_fire: bool,
    min_severity: String,
    category: String,
    #[serde(default)]
    detail_must_contain: Vec<String>,
    #[serde(default)]
    detail_must_not_contain: Vec<String>,
    #[serde(default)]
    suggestion_must_contain: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct AntiFinding {
    rule_id: String,
    must_not_fire: bool,
    reason: String,
}

const MANIFEST_DIR: &str = env!("CARGO_MANIFEST_DIR");

fn ogagila_dir() -> PathBuf {
    PathBuf::from(MANIFEST_DIR).join("lib/ogagila")
}

fn severity_rank(s: &str) -> u8 {
    // Higher rank = more severe. Severity's own Ord impl is inverted
    // (Critical = 0, Info = 2), so we cannot use it for "min_severity"
    // semantics directly — that would accept Info when min_severity=Warning.
    match s {
        "critical" => 2,
        "warning" => 1,
        "info" => 0,
        _ => 0,
    }
}

fn category_str(c: &ogexplain_core::analyzer::report::DiagnosticCategory) -> &'static str {
    use ogexplain_core::analyzer::report::DiagnosticCategory::*;
    match c {
        ScanEfficiency => "ScanEfficiency",
        JoinStrategy => "JoinStrategy",
        MemoryUsage => "MemoryUsage",
        SortEfficiency => "SortEfficiency",
        NetworkOverhead => "NetworkOverhead",
        CostMisestimation => "CostMisestimation",
        PushdownFailure => "PushdownFailure",
        TypeMismatch => "TypeMismatch",
        Vectorization => "Vectorization",
        SubqueryStructure => "SubqueryStructure",
        DistributionIssue => "DistributionIssue",
        General => "General",
    }
}

fn load_case(case_dir: &Path) -> (CaseManifest, ExpectedFile) {
    let toml_text = fs::read_to_string(case_dir.join("case.toml"))
        .unwrap_or_else(|e| panic!("read case.toml in {}: {}", case_dir.display(), e));
    let manifest: CaseManifest = toml::from_str(&toml_text)
        .unwrap_or_else(|e| panic!("parse case.toml in {}: {}", case_dir.display(), e));

    let json_text =
        fs::read_to_string(case_dir.join("expected.findings.json")).unwrap_or_else(|e| {
            panic!(
                "read expected.findings.json in {}: {}",
                case_dir.display(),
                e
            )
        });
    let expected: ExpectedFile = serde_json::from_str(&json_text).unwrap_or_else(|e| {
        panic!(
            "parse expected.findings.json in {}: {}",
            case_dir.display(),
            e
        )
    });

    (manifest, expected)
}

fn load_explain_text(dataset: &DatasetConfig) -> String {
    match dataset.source.as_str() {
        "ogagila" => load_ogagila_explain(dataset),
        "supplemental" => load_supplemental_explain(dataset),
        other => unimplemented!("dataset.source = {other} not implemented (pilot phase)"),
    }
}

fn load_supplemental_explain(dataset: &DatasetConfig) -> String {
    let rel = dataset
        .supplemental_file
        .as_deref()
        .expect("dataset.supplemental_file required when source = supplemental");
    let path = PathBuf::from(MANIFEST_DIR).join(rel);
    fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read supplemental explain {}: {}", path.display(), e))
}

fn load_ogagila_explain(dataset: &DatasetConfig) -> String {
    let version = dataset
        .ogagila_version
        .as_deref()
        .expect("dataset.ogagila_version required when source = ogagila");
    let ids = dataset
        .ogagila_query_ids
        .as_deref()
        .expect("dataset.ogagila_query_ids required when source = ogagila");
    assert!(
        ids.len() == 1,
        "static mode currently supports exactly one ogagila_query_id per case (got {ids:?})"
    );
    let path = ogagila_dir()
        .join("benchmark")
        .join(version)
        .join("explains")
        .join(format!("{}.explain", ids[0]));
    fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read ogagila explain {}: {}", path.display(), e))
}

fn extract_query_sql(raw: &str) -> String {
    raw.lines()
        .find(|line| {
            let t = line.trim_start();
            t.starts_with("SELECT")
                || t.starts_with("UPDATE")
                || t.starts_with("DELETE")
                || t.starts_with("INSERT")
                || t.starts_with("WITH")
        })
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "<unknown>".into())
}

fn source_desc(manifest: &CaseManifest) -> String {
    let d = &manifest.dataset;
    let ids = d
        .ogagila_query_ids
        .as_ref()
        .map(|v| v.join(", "))
        .unwrap_or_default();
    format!(
        "{} {} / {}",
        d.source,
        d.ogagila_version.as_deref().unwrap_or("?"),
        ids
    )
}

fn plan_summary(plan: &ExplainPlan) -> String {
    let mut parts: Vec<String> = Vec::new();
    let mut node: Option<&ogexplain_core::model::PlanNode> = Some(&plan.root);
    while let Some(n) = node {
        let label = match &n.relation {
            Some(rel) => format!("{} on {}", n.node_type, rel),
            None => n.node_type.to_string(),
        };
        parts.push(label);
        node = n.children.first();
    }
    parts.join(" → ")
}

#[derive(Default)]
struct CaseReport {
    case_name: String,
    #[allow(dead_code)]
    rule_id: String,
    source: String,
    query_sql: String,
    plan: String,
    positives: Vec<PositiveResult>,
    antis: Vec<AntiResult>,
    errors: Vec<String>,
}

#[derive(Default)]
struct PositiveResult {
    rule_id: String,
    #[allow(dead_code)]
    fired: bool,
    severity: Option<String>,
    category: Option<String>,
    detail: Option<String>,
    suggestion: Option<String>,
    ok: bool,
    failures: Vec<String>,
}

#[derive(Default)]
struct AntiResult {
    rule_id: String,
    reason: String,
    fired_count: usize,
    ok: bool,
}

fn validate_case(case_dir: &Path) -> CaseReport {
    let case_name = case_dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "<unknown>".into());
    let parent = case_dir
        .parent()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "?".into());

    let (manifest, expected) = load_case(case_dir);
    let raw = load_explain_text(&manifest.dataset);

    let mut report = CaseReport {
        case_name: case_name.clone(),
        rule_id: manifest.rule_id.clone(),
        source: source_desc(&manifest),
        query_sql: extract_query_sql(&raw),
        plan: String::new(),
        positives: Vec::new(),
        antis: Vec::new(),
        errors: Vec::new(),
    };

    let label = format!("{parent}/{case_name}");

    if manifest.case_name != case_name {
        report.errors.push(format!(
            "[{label}] case.toml case_name='{}' does not match directory name='{}'",
            manifest.case_name, case_name
        ));
        return report;
    }

    let plan = match parse(&raw) {
        Ok(p) => p,
        Err(e) => {
            report.errors.push(format!("[{label}] parse failed: {e:?}"));
            return report;
        }
    };
    report.plan = plan_summary(&plan);
    let config = manifest.config.clone().unwrap_or_default();
    let findings = analyze_with_config(&plan, &config).findings;

    for exp in &expected.findings {
        let matching: Vec<_> = findings
            .iter()
            .filter(|f| f.rule_id == exp.rule_id)
            .collect();
        let mut pr = PositiveResult {
            rule_id: exp.rule_id.clone(),
            fired: !matching.is_empty(),
            ..PositiveResult::default()
        };
        let mut local_failures: Vec<String> = Vec::new();

        if exp.must_fire && matching.is_empty() {
            local_failures.push(format!(
                "{} must_fire=true but no finding produced",
                exp.rule_id
            ));
        }
        if !exp.must_fire && !matching.is_empty() {
            local_failures.push(format!(
                "{} must_fire=false but {} finding(s) produced",
                exp.rule_id,
                matching.len()
            ));
        }

        for f in &matching {
            pr.severity = Some(f.severity.as_str().to_string());
            pr.category = Some(category_str(&f.category).to_string());
            pr.detail = Some(f.detail.clone());
            pr.suggestion = f.suggestion.clone();

            let actual_rank = severity_rank(f.severity.as_str());
            let min_rank = severity_rank(&exp.min_severity);
            if actual_rank < min_rank {
                local_failures.push(format!(
                    "{} severity {:?} below min_severity {}",
                    exp.rule_id, f.severity, exp.min_severity
                ));
            }

            let actual_cat = category_str(&f.category);
            if actual_cat != exp.category {
                local_failures.push(format!(
                    "{} category {actual_cat} != expected {}",
                    exp.rule_id, exp.category
                ));
            }

            for needle in &exp.detail_must_contain {
                if !f.detail.contains(needle.as_str()) {
                    local_failures.push(format!(
                        "{} detail missing '{needle}': {}",
                        exp.rule_id, f.detail
                    ));
                }
            }
            for needle in &exp.detail_must_not_contain {
                if f.detail.contains(needle.as_str()) {
                    local_failures.push(format!(
                        "{} detail unexpectedly contains '{needle}': {}",
                        exp.rule_id, f.detail
                    ));
                }
            }
            if let Some(s) = &f.suggestion {
                for needle in &exp.suggestion_must_contain {
                    if !s.contains(needle.as_str()) {
                        local_failures.push(format!(
                            "{} suggestion missing '{needle}': {}",
                            exp.rule_id, s
                        ));
                    }
                }
            }
        }

        pr.ok = local_failures.is_empty();
        if !pr.ok {
            report.errors.extend(local_failures);
        }
        report.positives.push(pr);
    }

    for anti in &expected.anti_findings {
        if !anti.must_not_fire {
            continue;
        }
        let count = findings
            .iter()
            .filter(|f| f.rule_id == anti.rule_id)
            .count();
        let ok = count == 0;
        if !ok {
            report.errors.push(format!(
                "[{label}] anti_finding {} must_not_fire but {count} fired. reason: {}",
                anti.rule_id, anti.reason
            ));
        }
        report.antis.push(AntiResult {
            rule_id: anti.rule_id.clone(),
            reason: anti.reason.clone(),
            fired_count: count,
            ok,
        });
    }

    report
}

fn print_case_report(report: &CaseReport) {
    let bar = "\u{2501}".repeat(78);
    println!("\n{} {}", bar, report.case_name);
    println!("  source:  {}", report.source);
    println!("  query:   {}", report.query_sql);
    println!("  plan:    {}", report.plan);
    println!();

    for pr in &report.positives {
        if pr.ok {
            let sev = pr.severity.as_deref().unwrap_or("?");
            let cat = pr.category.as_deref().unwrap_or("?");
            println!("  \u{2713} {} fires \u{2014} {} / {}", pr.rule_id, sev, cat);
            if let Some(d) = &pr.detail {
                println!("      detail:     {d:?}");
            }
            if let Some(s) = &pr.suggestion {
                println!("      suggestion: {s:?}");
            }
        } else {
            println!("  \u{2717} {} FAILED", pr.rule_id);
            for f in &pr.failures {
                println!("      {f}");
            }
        }
        println!();
    }

    if !report.antis.is_empty() {
        println!("  anti-findings (verified NOT fired):");
        for ar in &report.antis {
            let mark = if ar.ok { "\u{2713}" } else { "\u{2717}" };
            let extra = if ar.fired_count > 0 {
                format!(" (fired {} times!)", ar.fired_count)
            } else {
                String::new()
            };
            println!("      {mark} {} \u{2014} {}{extra}", ar.rule_id, ar.reason);
        }
    }
}

fn run_case_with_report(rel_path: PathBuf) {
    let case_dir = PathBuf::from(MANIFEST_DIR).join(&rel_path);
    let report = validate_case(&case_dir);
    print_case_report(&report);
    if !report.errors.is_empty() {
        panic!(
            "{} assertion(s) failed for {}:\n  - {}",
            report.errors.len(),
            report.case_name,
            report.errors.join("\n  - ")
        );
    }
}

include!(concat!(env!("OUT_DIR"), "/regress_generated_tests.rs"));
