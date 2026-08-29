# ogexplain-analyzer

## TDD 工作流（Red → Green → Refactor）

本仓库采用测试驱动开发。一次循环只锁定一个行为：先写会失败的测试（Red），再写最小实现让它通过（Green），最后在测试全绿的前提下重构（Refactor）。探索草稿不得直接合入，必须按本文件用 TDD 重写。

### 先读再改
1. 确认改动落在哪个 crate（本仓库是 Cargo workspace，见「仓库地图」）。
2. 只用本文件列出的 cargo 命令；不要发明裸 `cargo update`。本仓库**没有** `rust-toolchain.toml`，toolchain 以 CI 使用的 `stable` 为准，不要擅自切换或加 `+nightly`。
3. 先跑与改动相关的最小测试；提交前再跑 workspace 门禁（fmt + clippy + test）。
4. 完成一个循环后按「完成标准与汇报」汇报，不要只说「做完了」。

### Never / Ask first / Always

**Never（不必请示，直接禁止）**
- 删除、注释、跳过已有测试：`#[ignore]`、注释掉 `#[test]`、把断言改成 `is_ok()` / `unwrap()` 了事
- 修改人类已有测试的断言来迁就实现
- 先提交无测试的业务行为，再「回头补」
- 写永真测试：无断言、只检查 `is_some()`、只 verify 调用次数不查参数与状态
- 用全量端到端测试覆盖本可单测完成的改动
- 提交半成品；每次对人类可见的结果必须能构建且相关测试为绿
- 把探索草稿、临时脚本、调试 `dbg!`/`println!` 留在主代码

**Ask first**
- 改人类已有测试（含断言、fixture、snapshot）
- 新增运行时依赖、`unsafe`、新的 workspace crate、新的外部服务
- 为不可测代码做超出当前改动路径的重构
- 接受/更新 snapshot（insta / golden file）且行为含义发生变化
- 关闭 clippy lint、新增 `#[allow]`

**Always**
- 改遗留路径前：先写特征测试，锁定当前可观察行为（允许丑，必须可重复）
- 新行为：先有会失败的行为断言，再写最少实现
- 难以测试时：先造接缝，再写测试（见「遗留代码与接缝」）
- 测试名描述行为：`should_reject_negative_amount`
- 现有测试因你的改动失败：修实现，不修测试（除非人类明确要求）

测试权限：

| 测试来源 | 权限 |
|---|---|
| 人类已有测试 | 只读 |
| 本任务新建测试 | 可改，直到该行为稳定 |
| 过时或环境偶发失败 | 只报告，不擅自跳过 |

### 工作流

**Red** — 写生产行为之前先写测试；测试必须能被收集且必须失败（断言失败，或因缺失 API 导致编译失败，二者都算合法 Red）。修改已有功能先写特征测试锁定当前输出。一次只加一个行为的测试，禁止一批 20 个用例再一次性实现。

**Green** — 只写让当前失败测试通过的最少代码。禁止删掉/改掉失败测试、一次引入多个未验证变更、用更宽断言或 `unwrap()` 换绿。

**Refactor** — 相关测试全绿后才重构；重构后立刻跑同一组测试；范围限于当前 crate，不扩散到无关 crate。

**探索 vs 实现** — 需求或方案不清可写草稿验证；草稿不得合并；方案确定后必须走 TDD 重写。

### 遗留代码与接缝

**特征测试** — 锁定现有行为，不是证明它正确。用固定 fixture 或 `insta` snapshot。更新 snapshot 必须在汇报里写清 diff 含义；默认不接受「看起来差不多」。

**接缝（优先顺序，靠后的更差）**
1. trait + 泛型或 `impl Trait`，测试用假类型
2. 用类型去掉非法状态（enum / newtype），而不是在测试里补分支
3. 时钟、ID、熵、文件系统做成可注入依赖；测试用 `tempfile` / 内存实现
4. `unsafe` 不是接缝。新增 `unsafe` 必须 Ask first，并写 `SAFETY` 注释

只给即将修改的代码路径补测试，不要一次性给整个模块「补全覆盖率」。

### 测试分层

| 层级 | 位置 | 测什么 |
|---|---|---|
| 单元 | `src` 内 `#[cfg(test)] mod tests` | 模块不变量、错误类型、状态转换 |
| 集成 | `tests/*.rs` | 公共 API；不可访问私有项 |
| 文档测试 | `///` 示例 | 公共 API 必须可运行；禁止滥用 `no_run` |
| CLI/二进制 | 项目惯用方式 | 退出码与 stdout 契约 |
| 不变量 | `proptest`（项目已用时） | 往返解析、幂等、单调性 |
| 特征/快照 | `insta` 或固定 fixture | 遗留输出；接受 snapshot 必须说明 |

不要把本该测公共契约的内容塞进 `#[cfg(test)]` 去读私有字段。

Rust 的 Red 允许是：测试引用了尚不存在的类型/函数导致编译失败。不要为了先编译而写空 `todo!()` 实现再补测试——可以留 `todo!()` 仅作为 Green 的最小占位，且下一步必须替换。

### Rust Never 补遗
- 库代码（非 main/example/测试）用 `unwrap` / `expect` / `panic!` 做控制流
- 无必要 `unsafe`；有则必须 `SAFETY` 注释
- 一次性 `cargo update` 整个 lockfile
- 用 `#[allow(...)]` 静默应修复的 lint
- 为绿而改 snapshot 却不解释行为是否应该变

### 命令

```bash
# 单测（core 库，按测试名过滤）
cargo test -p ogexplain-core <test_name>

# 单 crate
cargo test -p ogexplain-core

# 解析器 snapshot 测试
cargo test --test integration_tests

# 分析器诊断测试
cargo test --test analyzer_tests

# 提交前门禁（本仓库没有测试 CI，必须本地跑完）
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace

# snapshot 审阅（仅当行为含义变化且已 Ask first 才接受）
cargo insta review
```

> ⚠️ **本仓库 `.github/workflows/` 只有 `release.yml`，没有 PR/push 的测试 CI。**以上门禁没有 CI 兜底，必须本地逐条跑完，并在汇报里贴出实际命令与结果。

`ogexplain-core` 是纯库（无 UI/IO 依赖），parser 永不 panic（返回 Result），诊断规则可独立测试。

循环内只跑受影响 crate；提交前再 workspace。

### 完成标准与汇报

提交或交还人类前，确认：
- [ ] 新行为有失败→通过的测试
- [ ] 修改的遗留路径有特征测试
- [ ] 未删除、跳过、改写人类已有测试
- [ ] 已跑与改动匹配的门禁（fmt + clippy + test）
- [ ] `cargo fmt` 与 clippy 干净
- [ ] 没有把草稿、调试输出、无主 lockfile 大面积变更带上

每个 TDD 循环汇报：
1. 测试了什么行为（测试函数名）
2. 最小实现改了哪些文件
3. 是否重构、边界在哪
4. 实际执行的命令和结果（通过 / 失败原因；不要只写「测过了」）

### 质量判断（自我检查）
- 这条测试在实现写错时会失败吗？
- 我是否在测行为，而不是私有实现细节？
- 我是否用 skip、更宽断言、unwrap、snapshot 盲收换绿？
- 命令是否来自本文件，而不是我编的？


## What This Is

OpenGauss `EXPLAIN` / `EXPLAIN ANALYZE` output parser and performance diagnostics tool. Parses TEXT-format explain plans, runs diagnostic rules (OG-specific checks for pushdown, vectorization, streaming, implicit type coercion), and outputs findings + optimization suggestions.

**Status: Phases 1–3 complete, Phase 4 (polish) in progress.** Core parser, diagnostic engine, CLI, and TUI are all functional. 317 tests pass, zero clippy warnings. 25 diagnostic rules with parameterized suggestions and shared utility layer.

## Project Structure

```
.sisyphus/plans/ogexplain-analyzer-spec.md   # Design spec (1823 lines)
.sisyphus/plans/ogexplain-analyzer-impl.md   # Implementation plan (phases, deps, TUI design)
GaussDB-2.23.07.210/                         # GaussDB product docs reference data
  sql_plan_hints.json                        # 53 SQL plan hints extracted from docs
  term/                                      # 457+ JSON term files from product documentation
lib/openGauss-server/                        # Git submodule (openGauss source, gitignored)
Cargo.toml                                   # Workspace root
crates/
  ogexplain-core/                            # Core library (model + parser + analyzer + suggester)
  ogexplain-optimizer/                       # Closed-loop optimizer (orchestrator + converge + rewrite/verify)
  ogexplain-cli/                             # CLI frontend
  ogexplain-tui/                             # Interactive TUI frontend
  ogexplain-mcp/                             # MCP server for AI assistants
tests/
  fixtures/                                  # 31 EXPLAIN TEXT fixture files (01–31 + complex)
  integration_tests.rs                       # Parser insta snapshot tests
  analyzer_tests.rs                          # Analyzer diagnostic tests
```

- `lib/openGauss-server` is a git submodule pointing to `https://gitee.com/opengauss/openGauss-server`. It is **gitignored** — it exists only as local reference for source code analysis, not as a build dependency.
- The `.gitignore` also ignores `/target` (Rust convention) and `/examples/gauss`.

## Architecture

Rust Cargo workspace with six crates:

| Crate | Binary | Purpose |
|-------|--------|---------|
| `ogexplain-core` | library | Parser + Model + Analyzer + Suggester (no UI deps) |
| `ogexplain-optimizer` | library | Closed-loop optimizer — orchestrator, convergence, rewrite/verify integration with metamorphosis |
| `ogexplain-cli` | `ogexplain` | CLI frontend — file/pipe input, text/JSON/CSV output, `optimize` subcommand, config file for DB connections (`~/.gaussdb-mcp.toml`) |
| `ogexplain-tui` | `ogexplain-tui` | Interactive TUI — collapsible plan tree, node detail, paste input |
| `ogsql-complexity` | library | SQL complexity scoring (standalone, reusable) |
| `ogexplain-mcp` | `ogexplain-mcp` | MCP server — exposes analysis as MCP tools for AI assistants |

### Core layers:

1. **Parser** (`parser/`): Two-phase — line classifier (regex per line) → tree builder (indent-based stack). Handles pretty mode (`N --` prefix), `using <index>` clause, unknown nodes, missing actual stats, `(Actual time: never executed)`.
2. **Model** (`model/`): `ExplainPlan` → `PlanNode` tree with `NodeType` enum (80+ variants including `Vector*`, `CStore*`, `Streaming`, `Partitioned*`), `EstimatedCost`, `ActualStats`, `BufferStats`, `NodeProperty`. All types `#[derive(Serialize)]`.
3. **Analyzer** (`analyzer/`): Rule engine via `DiagnosticRule` trait + `DiagnosticEngine` with DFS traversal. 25 rules implemented across 17 rule files with shared utility layer (`rules/utils.rs`). `DiagnosticConfig` with configurable thresholds and disabled rules support.
4. **Suggester** (`suggester/`): Maps diagnostic findings to actionable suggestions with cross-rule synthesis patterns (multi-spill → work_mem, multi-estimation → stale stats, scan+join → composite index, type consistency, engine unification).
5. **SQL block parser** (`sql/`): Segments mixed SQL + EXPLAIN text into blocks for batch processing.
6. **Summary** (`summary/`): SummaryRow for batch reporting with SQL complexity + plan metrics + diagnostic stats.
7. **i18n** (`i18n/`): rust-i18n based localization (en, zh-CN).

### Optimizer layers:

6. **Orchestrator** (`orchestrator/`): Main loop — `EXPLAIN → diagnose → map → rewrite → verify → converge`. Uses `ExplainExecutor` trait for DB injection.
7. **Converge** (`converge/`): `MetricsSnapshot`, `LoopConfig`, `StopReason`, `should_continue()`.
8. **Mapper** (`mapper/`): `map_diagnostic()`, `filter_rewritable()`, `RemediationAction`.
9. **Rewrite** (`rewrite/`): SQL↔AST encapsulation for `metamorphosis_core::RewriteEngine`.
10. **Verify** (`verify.rs`): `verify_qed()` (embedded Z3), `verify_verieql()` (bounded MCF).

### TUI (ratatui + Elm Architecture):

- Custom tree rendering with severity icons (🔴🟡🔵) and expand/collapse
- `ratatui-textarea` for multi-line EXPLAIN paste input
- TEA pattern: Event → Action → Model mutation → Render
- Panels: TreePanel | DetailPanel / InputPanel | StatusBar
- Input modes: `ogexplain-tui file.txt` (load file) or `ogexplain-tui` (paste mode, Ctrl+P to parse)
- Key bindings: Ctrl+P parse, Tab cycle focus, ↑↓ navigate, Enter expand/collapse, q quit

### Key files:

| File | What it is |
|------|-----------|
| `crates/ogexplain-core/src/lib.rs` | Public API: `parse()`, `analyze()`, `analyze_with_config()` |
| `crates/ogexplain-core/src/model/` | Data model — `plan.rs`, `node_type.rs`, `join_type.rs`, `streaming.rs`, `cost.rs`, `buffer.rs` |
| `crates/ogexplain-core/src/parser/` | `mod.rs`, `line_classifier.rs`, `tree_builder.rs` |
| `crates/ogexplain-core/src/analyzer/` | `mod.rs`, `config.rs`, `context.rs`, `report.rs`, `rules/*.rs` (17 files incl. `utils.rs`) |
| `crates/ogexplain-core/src/suggester/` | `mod.rs`, `suggestion.rs`, `mapper.rs` |
| `crates/ogexplain-cli/src/main.rs` | clap CLI with `analyze` and `optimize` subcommands |
| `crates/ogexplain-tui/src/` | `main.rs`, `app.rs` (TEA model), `action.rs`, `event.rs`, `components/` |
| `crates/ogexplain-mcp/src/` | `main.rs`, `server.rs` — MCP server with 5 tools (`analyze_explain`, `parse_explain`, `list_diagnostic_rules`, `get_suggestions`, `score_sql_complexity`) |
| `crates/ogexplain-optimizer/src/orchestrator.rs` | Main optimization loop |
| `crates/ogexplain-optimizer/src/converge.rs` | Convergence detection |
| `crates/ogexplain-optimizer/src/verify.rs` | QED/VeriEQL library API integration |
| `crates/ogexplain-optimizer/src/rewrite.rs` | SQL↔AST encapsulation |
| `crates/ogexplain-optimizer/src/mapper.rs` | Diagnostic→rule mapping |
| `.sisyphus/plans/ogexplain-analyzer-spec.md` | Design spec (1823 lines) |
| `.sisyphus/plans/ogexplain-analyzer-impl.md` | Implementation plan |

### Implemented Diagnostic Rules (25 of 45+ planned)

| ID | Rule | Category |
|----|------|----------|
| SCAN-001 | Large table full scan | scan |
| SCAN-004 | Filter without index | scan |
| JOIN-001 | Nested loop on large tables | join |
| JOIN-002 | Hash join spill to disk | join |
| MEM-001 | Sort spill to disk | memory |
| MEM-004 | High peak memory | memory |
| SORT-003 | Duplicate sort | sort |
| NET-001 | Broadcast large data | network |
| EST-001 | Severe row estimation error | estimation |
| EST-004 | Nested loop from underestimation | estimation |
| PUSH-001 | Query not pushed down | pushdown |
| PUSH-002 | Multi-layer streaming | pushdown |
| TYPE-001 | Implicit type coercion | type_coercion |
| TYPE-004 | LIKE with leading wildcard | type_coercion |
| VEC-001 | Mixed row/vector engines | vectorization |
| GEN-001 | Plan too deep | general |
| SUBQ-001 | Subquery not pulled up | subquery |
| REW-001 | Large IN list not rewritten | subquery |
| SUBQ-006 | Correlated subquery self-update | subquery |
| AGG-001 | Group aggregate should be hash | aggregate |
| AGG-002 | Hash aggregate spill to disk | aggregate |
| SKEW-001 | Data skew detected | distribution |
| DIST-001 | Distribution column mismatch | distribution |
| STATS-001 | Stats not collected | stats |
| PART-001 | Partition pruning failure | partition |

## Key Domain Knowledge

This tool targets **OpenGauss** (PostgreSQL-fork), not vanilla PostgreSQL. OG-specific EXPLAIN features that the parser handles:

- **Vector nodes**: `Vector Hash Join`, `Vec Sort`, `Vector Sonic Hash Join/Aggregate`, etc.
- **CStore nodes**: `CStore Scan`, `CStore Index Scan`, columnar storage scans.
- **Streaming nodes**: `Streaming(type: GATHER|REDISTRIBUTE|BROADCAST|...)` with DOP (`dop: c/p`) and NodeGroup (`ng: g1->g2`) info.
- **Pushdown**: FQS (Fast Query Shipping) detection via absence/presence of Streaming nodes; `Data Node Scan` + `Remote query` = successful pushdown.
- **Implicit type coercion**: OG hides implicit casts in EXPLAIN (`showimplicit=false`); must detect via indirect patterns (Seq Scan + Filter + high Rows Removed).
- **Adapters**: `Row Adapter` / `Vector Adapter` mark row↔vector engine boundaries.
- **Pretty mode**: Node IDs with `--` prefix, detailed per-node runtime stats.
- **OG-specific properties**: Bloom Filter info, Min/Max skip, DFS file pruning, LLVM optimization markers, Skew optimization markers, CPU details, Dynamic SMP, AI prediction (`p-time`, `p-rows`).

## Dependencies

- **core**: `regex`, `serde` + `serde_json`, `thiserror`, `toml`; dev: `insta` (YAML snapshots)
- **optimizer**: `ogexplain-core`, `metamorphosis-core`, `metamorphosis-rewrite`, `z3` (via QED), `tokio`
- **cli**: `ogexplain-core`, `clap` v4, `colored`, `anyhow`
- **tui**: `ogexplain-core`, `ratatui` 0.30, `crossterm` 0.29, `ratatui-textarea` 0.8, `tokio`, `color-eyre`, `clap` v4
- **mcp**: `ogexplain-core`, `ogsql-complexity`, `rmcp` 1.7 (official MCP SDK), `tokio`, `serde`, `schemars`

## Build & Test

```bash
cargo build                              # full workspace
cargo build -p ogexplain-core            # core library only
cargo test --workspace                   # all 317 tests
cargo test --test integration_tests      # parser insta snapshot tests only
cargo test --test analyzer_tests         # analyzer diagnostic tests only
cargo test -p ogexplain-optimizer        # optimizer library tests
cargo test --test optimize_e2e           # optimizer end-to-end
cargo test --test optimize_regress       # optimizer regression tests
cargo insta review                       # interactive snapshot review
cargo run -p ogexplain-cli -- analyze file.txt -o json   # CLI (text or json output)
cargo run -p ogexplain-tui -- file.txt   # TUI with file
cargo run -p ogexplain-tui               # TUI paste mode
cargo run -p ogexplain-mcp               # MCP server (stdio transport)
cargo fmt --all && cargo clippy --workspace  # lint (zero warnings)
```

Test fixtures go in `tests/fixtures/` — each is a raw EXPLAIN TEXT output file. Each fixture has a corresponding `insta::assert_yaml_snapshot!` test.

## Constraints

- `ogexplain-core` is a pure library — no UI/IO crate dependencies (no ratatui, no crossterm, no clap).
- All model types derive `Serialize`.
- Parser never panics — returns `Result<_, ParseError>`.
- Diagnostic rules are independently testable via `DiagnosticRule::check()`.
- TUI state and rendering are strictly separated (TEA architecture).

## OpenGauss Source Reference

The spec references specific source files in `lib/openGauss-server/` for parsing behavior:
- `src/gausskernel/optimizer/commands/explain.cpp` — EXPLAIN output generation
- `src/gausskernel/optimizer/util/optcommon.cpp` — plan node naming
- `src/gausskernel/optimizer/plan/pgxcship.cpp` — pushdown/shippability logic
- `src/gausskernel/runtime/executor/indxpath.cpp` — index path selection (implicit cast detection)
- `src/gausskernel/optimizer/commands/plananalyzer.cpp` — unpushable query analysis

These paths are relative to `lib/openGauss-server/`.

## MCP Server

The `ogexplain-mcp` binary exposes 5 tools via the Model Context Protocol (stdio transport):

| Tool | Description |
|------|-------------|
| `analyze_explain` | Parse + analyze EXPLAIN plan → diagnostic findings (JSON + text) |
| `parse_explain` | Parse EXPLAIN text → structured plan tree (JSON) |
| `list_diagnostic_rules` | List all 25 diagnostic rules with IDs and descriptions |
| `get_suggestions` | Cross-rule synthesis suggestions (work_mem, composite index, etc.) |
| `score_sql_complexity` | SQL complexity scoring (standard 0–100 + GaussDB 4-dimension) |

Configure in Claude Desktop / Cursor / VS Code:

```json
{
  "mcpServers": {
    "ogexplain": {
      "command": "ogexplain-mcp",
      "args": []
    }
  }
}
```

Works with `gaussdb-mcp` for end-to-end SQL performance diagnostics: gaussdb-mcp runs `EXPLAIN` → ogexplain-mcp analyzes the result.

## Remaining Work (Optional / Incremental)

- Remaining 20+ diagnostic rules (from 25 to 45+) — see spec for full list
- Markdown output for CLI (`-o markdown`)
- TOML config file loading (`--config file.toml`)
- `suggester/synthesizer.rs` full implementation
