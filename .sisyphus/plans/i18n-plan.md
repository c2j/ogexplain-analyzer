# i18n Plan: Locale-Aware Bilingual Support (zh-CN / en)

## Goal

Auto-detect system locale at startup. If Chinese → Chinese help + TUI + CLI output. Otherwise → English. Support `--lang` override flag.

## Architecture

**Approach: `rust-i18n` crate with centralized locale files in core crate.**

- Locale files live in `crates/ogexplain-core/i18n/` (en.yml, zh-CN.yml)
- Core crate owns locale detection + initialization via `sys-locale`
- CLI and TUI crates point to core's i18n dir via `rust_i18n::i18n!("../ogexplain-core/i18n")`
- `rust_i18n::set_locale()` is process-global — set once in each binary's `main()`

## Key Naming Convention

Flat dot-separated keys organized by crate/component:
```
cli.about                          # "OpenGauss EXPLAIN plan analyzer"
cli.analyze.about                  # "Analyze an EXPLAIN output file"
cli.analyze.help_file              # "Path to EXPLAIN output file..."
cli.report.header                  # "OpenGauss Execution Plan Analysis Report"
cli.findings.critical              # "Critical ({count})"
tui.help.title                     # "快捷键帮助 (?) 关闭"
tui.help.nav.up                    # "上移"
tui.detail.section.node            # "── 节点 ──"
tui.detail.label.type              # "类型: "
core.rule.gen_001.suggestion       # "计划树过深..."
```

## Implementation Phases

### Phase 1: Infrastructure (blocking — do first)

1. **Add dependencies:**
   - `crates/ogexplain-core/Cargo.toml`: add `rust-i18n = "3"`, `sys-locale = "0.3"`
   - `crates/ogexplain-cli/Cargo.toml`: add `rust-i18n = "3"`
   - `crates/ogexplain-tui/Cargo.toml`: add `rust-i18n = "3"`

2. **Create i18n module in core** (`crates/ogexplain-core/src/i18n.rs`):
   ```rust
   pub fn detect_locale() -> String {
       let lang = sys_locale::get_locale().unwrap_or_else(|| "en".to_string());
       if lang.starts_with("zh") { "zh-CN".to_string() } else { "en".to_string() }
   }
   pub fn init(locale: Option<&str>) {
       let loc = locale.unwrap_or_else(|| Box::leak(detect_locale().into_boxed_str()));
       rust_i18n::set_locale(loc);
   }
   ```

3. **Add `rust_i18n::i18n!("i18n")` to core's lib.rs** — export `t!()` macro
4. **Add `rust_i18n::i18n!("../ogexplain-core/i18n")` to CLI and TUI lib.rs**

5. **Create locale file skeletons:**
   - `crates/ogexplain-core/i18n/en.yml` — English strings (230+ keys)
   - `crates/ogexplain-core/i18n/zh-CN.yml` — Chinese strings (current hardcoded values)

6. **Add `--lang` arg to CLI and TUI** (values: "en", "zh-CN", default: auto-detect)

### Phase 2: CLI String Replacement

Files to modify: `crates/ogexplain-cli/src/lib.rs`

Replace all ~100 hardcoded strings:

| Section | Strings | Pattern |
|---------|---------|---------|
| Clap help (9) | `#[command(about)]`, `#[arg(help)]` | Use `Command::mut_arg()` after building from derive, before parsing |
| Report header (3) | "OpenGauss Execution Plan..." | `t!("cli.report.header")` |
| Findings labels (8) | "Critical", "Warnings", "Info" | `t!("cli.findings.critical", count = n)` |
| Finding detail (4) | "Node:", "Suggestion:" | `t!("cli.finding.node", ...)` |
| Plan tree (10) | "Plan Tree", "cost=", "actual=" | `t!("cli.tree.cost", ...)` |
| Summary table (37) | Column headers | `t!("cli.summary.col_tables")` |
| Complexity (30+) | "GaussDB评分:", "类型:", "维度:" | `t!("cli.complexity.gauss_score")` |
| Error messages (5) | "Failed to parse..." | `t!("cli.error.parse_failed")` |

**Clap help localization strategy:**
```rust
pub fn run() -> Result<()> {
    // Parse --lang early (before full clap parse)
    let locale = detect_and_set_locale();
    
    // Build localized CLI from derive + mutations
    let mut cmd = Cli::into_app();
    cmd = cmd.about(t!("cli.about"));
    // ... mut_arg for each arg's help text
    let matches = cmd.get_matches();
    // ... rest of logic
}
```

### Phase 3: TUI String Replacement

Files to modify (6 files):

| File | Strings | 
|------|---------|
| `components/help_overlay.rs` | ~30 shortcut descriptions |
| `components/status_bar.rs` | ~25 status hints |
| `components/detail_panel.rs` | ~65 section headers + labels |
| `components/summary_bar.rs` | ~12 labels |
| `components/tree_panel.rs` | ~3 labels |
| `app.rs` | ~14 error/status strings |

Replace each `Span::styled("上移", ...)` → `Span::styled(t!("tui.help.nav.up"), ...)`

### Phase 4: Core Rule String Replacement (Optional — Lower Priority)

Files to modify:
- `crates/ogexplain-core/src/analyzer/rules/*.rs` (10 files) — Finding title/detail/suggestion
- `crates/ogexplain-core/src/suggester/mapper.rs` — Suggestion messages

Keys: `core.rule.scan_001.title`, `core.rule.scan_001.detail`, `core.rule.scan_001.suggestion`

**Note:** Core strings are currently mixed Chinese/English. Phase 4 makes them bilingual. Can defer.

## Execution Strategy

Phase 1 (infrastructure) is blocking — must complete before Phase 2/3.

Phase 2 (CLI) and Phase 3 (TUI) are independent — run in parallel after Phase 1.

Phase 4 is optional — can defer.

## Testing

1. `LANG=zh_CN.UTF-8 cargo run --bin ogexplain -- analyze file.txt` → Chinese output
2. `LANG=en_US.UTF-8 cargo run --bin ogexplain -- analyze file.txt` → English output
3. `cargo run --bin ogexplain -- --lang zh-CN -- help` → Chinese help
4. `cargo run --bin ogexplain -- --lang en -- help` → English help
5. `cargo test --workspace` → all existing tests still pass
6. TUI: verify both locales render correctly

## Risks

- **Clap derive + runtime i18n**: derive attributes are compile-time. Must use `Command::mut_arg()` for runtime help text. Alternative: switch to builder pattern (more disruptive).
- **`rust-i18n` cross-crate paths**: relative path `"../ogexplain-core/i18n"` must resolve from CLI/TUI crate roots. Test early.
- **Format strings**: Some strings use positional args (e.g., `"命中={} 读取={}"`). `rust-i18n` uses `%{var}` syntax for interpolation. All format strings must be converted.
- **TUI layout width**: Chinese text is wider than English. Status bar and help overlay layouts may need width adjustments for English locale.
