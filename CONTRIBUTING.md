# Contributing to ogexplain-analyzer

Thank you for your interest in contributing! This document outlines the development workflow, coding standards, and how to add new features.

## Development Setup

```bash
git clone https://github.com/c2j/ogexplain-analyzer.git
cd ogexplain-analyzer
cargo build --workspace
cargo test --workspace
```

**Requirements:** Rust 2021 edition, Cargo.

**Recommended tools:**
- `cargo fmt` — Code formatting (mandatory)
- `cargo clippy` — Linting (zero warnings required)
- `cargo insta` — Snapshot test review

## Project Structure

```
ogexplain-analyzer/
├── crates/
│   ├── ogexplain-core/      # Core library: parser, model, analyzer, suggester, rewriter
│   ├── ogexplain-cli/       # CLI binary: analyze, explain, mcp subcommands
│   ├── ogexplain-tui/       # TUI binary: interactive plan browser
│   ├── ogexplain-mcp/       # MCP server: AI assistant integration
│   └── ogsql-complexity/    # SQL complexity scoring library
├── tests/
│   ├── fixtures/            # 31 EXPLAIN TEXT test fixtures
│   ├── integration_tests.rs # Parser snapshot tests
│   └── analyzer_tests.rs    # Diagnostic rule tests
└── docs/                    # Documentation
    └── plans/               # Feature design plans
```

## Development Workflow

### 1. Choose an Issue / Feature

Check the [implementation plan](.sisyphus/plans/ogexplain-analyzer-impl.md) for remaining work items (Phase 4: remaining 20+ diagnostic rules, markdown output, TOML config).

### 2. Create a Branch

```bash
git checkout -b feat/my-feature
# or
git checkout -b fix/my-bugfix
```

### 3. Write Tests First

**Parser changes**: Add a fixture file to `tests/fixtures/` and an `insta::assert_yaml_snapshot!` test in `tests/integration_tests.rs`.

**New diagnostic rule**: Write a positive test (plan that SHOULD trigger the rule) and a negative test (plan that SHOULD NOT).

**Example:**

```rust
// tests/analyzer_tests.rs
#[test]
fn test_my_new_rule_triggers() {
    let plan = parse(include_str!("fixtures/99_my_scenario.txt")).unwrap();
    let report = analyze(&plan);
    assert!(report.findings.iter().any(|f| f.rule_id == "NEW-001"));
}

#[test]
fn test_my_new_rule_no_false_positive() {
    let plan = parse(include_str!("fixtures/01_simple_seq_scan.txt")).unwrap();
    let report = analyze(&plan);
    assert!(!report.findings.iter().any(|f| f.rule_id == "NEW-001"));
}
```

### 4. Implement

Follow the existing patterns (see sections below).

### 5. Verify

```bash
cargo fmt --all
cargo clippy --workspace          # Must be zero warnings
cargo test --workspace            # All tests must pass
cargo test --test integration_tests
cargo test --test analyzer_tests
```

### 6. Submit a PR

- PR title: `feat:` / `fix:` / `docs:` / `refactor:` prefix
- Description: What changed, why, how to verify
- Link to related issue if applicable

## Coding Standards

### Rust Conventions

- `cargo fmt` for all code (non-negotiable)
- `cargo clippy --workspace` must produce **zero warnings**
- No `unwrap()` in library code — use `Result` with `thiserror`
- No `as any`, `@ts-ignore`, or type suppression
- No `unsafe` blocks without `// SAFETY:` comments
- Public API must have doc comments (`cargo doc` no warnings)

### Module Organization

- Single `.rs` file ≤ 600 lines (ideally ≤ 400)
- `core` layer must have zero IO/UI dependencies
- `pub(crate)` visibility only when strictly needed
- Follow existing module layout conventions

### Naming

- `verb_noun` naming consistently (e.g., `parse_text`, `build_tree`)
- No `get_` prefix on getters (use `name()` not `get_name()`)
- Type conversion: `as_` (borrow), `to_` (allocate), `into_` (consume)

## Adding a Diagnostic Rule

### Step 1: Understand the Rule Trait

```rust
pub trait DiagnosticRule: Send + Sync {
    fn id(&self) -> &str;
    fn name(&self) -> &str;
    fn severity(&self) -> Severity;
    fn category(&self) -> DiagnosticCategory;

    /// Check a single plan node. Return Some(Finding) if triggered.
    fn check(&self, node: &PlanNode, ctx: &PlanContext) -> Option<Finding>;

    /// Global check across the whole plan. Return Vec<Finding>.
    fn check_global(&self, plan: &ExplainPlan, stats: &GlobalStats) -> Vec<Finding>;
}
```

### Step 2: Create the Rule File

Create a new file in `crates/ogexplain-core/src/analyzer/rules/` (e.g., `my_rules.rs`).

### Step 3: Implement the Rule

```rust
pub struct MyNewRule;

impl DiagnosticRule for MyNewRule {
    fn id(&self) -> &str { "CAT-001" }

    fn name(&self) -> &str { "My new rule" }

    fn severity(&self) -> Severity { Severity::Warning }

    fn category(&self) -> DiagnosticCategory { DiagnosticCategory::General }

    fn check(&self, node: &PlanNode, ctx: &PlanContext) -> Option<Finding> {
        // Only trigger for specific node types
        if node.node_type != NodeType::SeqScan {
            return None;
        }

        // Check condition
        let estimated_rows = node.estimated.as_ref()?.plan_rows;
        if estimated_rows < 10000.0 {
            return None;
        }

        // Build finding
        Some(Finding {
            rule_id: self.id().to_string(),
            severity: self.severity(),
            category: self.category(),
            title: self.name().to_string(),
            detail: format!("Table {} has {} estimated rows", table_name, estimated_rows),
            node_line: Some(node.line_number),
            node_type: Some(node.node_type.to_string()),
            suggestion: Some("Consider adding an index".to_string()),
            sql_rewrite: None,
            evidence: None,
        })
    }

    fn check_global(&self, _plan: &ExplainPlan, _stats: &GlobalStats) -> Vec<Finding> {
        vec![]
    }
}
```

### Step 4: Register the Rule

In `crates/ogexplain-core/src/analyzer/rules/mod.rs`, add your rule to the `all_rules()` function:

```rust
pub fn all_rules(config: &DiagnosticConfig) -> Vec<Box<dyn DiagnosticRule>> {
    let mut rules: Vec<Box<dyn DiagnosticRule>> = vec![
        Box::new(scan_rules::LargeTableFullScan::default()),
        // ... existing rules ...
        Box::new(my_rules::MyNewRule),  // ADD HERE
    ];
    rules.retain(|r| !config.disabled_rules.contains(&r.id().to_string()));
    rules
}
```

### Step 5: Write Tests

Add positive and negative tests in `tests/analyzer_tests.rs`. Create a test fixture if needed.

### Step 6: Add Rule Metadata

Update the rule list in `crates/ogexplain-mcp/src/server.rs`'s `list_diagnostic_rules` tool.

## Testing Guidelines

### Parser Tests (integration_tests.rs)

- Every new node type or parsing feature needs a fixture + snapshot test
- Use `insta::assert_yaml_snapshot!` for regression testing
- Review snapshots with `cargo insta review`

### Diagnostic Rule Tests (analyzer_tests.rs)

- **Positive test**: Rule triggers on appropriate plan → assert finding exists
- **Negative test**: Rule does NOT trigger on irrelevant plan → assert finding absent
- Test edge cases: empty plan, single node, deeply nested, missing stats

### Running Specific Tests

```bash
cargo test -p ogexplain-core -- test_my_new_rule
cargo test --test analyzer_tests -- my_new_rule
cargo test --test integration_tests -- my_fixture
```

## Commit Convention

Use conventional commit prefixes:

- `feat:` — New feature (diagnostic rule, output format, subcommand)
- `fix:` — Bug fix
- `docs:` — Documentation changes
- `test:` — Adding or updating tests
- `refactor:` — Code refactoring (no behavior change)
- `chore:` — Build, CI, dependency updates

Example:
```
feat: add SCAN-005 bitmap scan detection rule

Detects cases where Bitmap Heap Scan on small tables
would be better as a plain Seq Scan. Includes positive
and negative tests.
```

## Code Review Checklist

Before submitting a PR, verify:

- [ ] `cargo fmt --all` passes
- [ ] `cargo clippy --workspace` — zero warnings
- [ ] `cargo test --workspace` — all tests pass
- [ ] No `unwrap()` in library code
- [ ] No type suppression (`as any`, `@ts-ignore`)
- [ ] New public items have doc comments
- [ ] New diagnostic rule has positive and negative tests
- [ ] Rule metadata updated in MCP server's `list_diagnostic_rules`
- [ ] No commented-out code left behind

## Questions?

- Check `.sisyphus/plans/ogexplain-analyzer-spec.md` for detailed design specs
- Check `.sisyphus/plans/ogexplain-analyzer-impl.md` for implementation plan and remaining work
- Check `AGENTS.md` for AI agent guidance
- Refer to `docs/CONTRIBUTING.md` for mandatory Rust coding standards (Chinese)
- Refer to `docs/BEST-PRATICE.md` for recommended Rust best practices (Chinese)
