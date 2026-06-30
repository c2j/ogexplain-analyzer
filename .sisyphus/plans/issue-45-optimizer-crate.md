# Issue #45 — `ogexplain-optimizer` Library Crate

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** 新建 `ogexplain-optimizer` library crate，将闭环优化编排逻辑从 `ogexplain-cli/src/optimize/` 和 `ogexplain-core/src/convergence.rs` 收敛到此 crate；同时将 metamorphosis 集成从子进程调用切换为 library API（`metamorphosis-core` / `metamorphosis-qed` / `metamorphosis-verieql`）；CLI 保留为 thin wrapper。

**Architecture:** 六阶段递增 — (0) 前置验证 + skeleton；(1) converge + mapper 迁移；(2) verify.rs 重写为库 API；(3) rewrite 封装层 + orchestrator DB 注入；(4) CLI 精简化；(5) 测试更新 + 回归验证。每阶段独立可提交。

**Tech Stack:** Rust, ogexplain-core, metamorphosis-core v0.1.29, metamorphosis-qed v0.1.29, metamorphosis-verieql v0.1.0, metamorphosis-rules v0.1.29, ogsql-parser, serde, thiserror, tracing.

**References:**
- Issue: https://github.com/c2j/ogexplain-analyzer/issues/45
- Metamorphosis workspace: `lib/metamorphosis/` (git submodule, **只读**)
- Pilot plan: `.sisyphus/plans/2026-06-28-closed-loop-pilot.md`
- Heptadecagon design: `docs/closed-loop-optimization-design.md`

---

## 0. 前置与依赖状态

### 0.1 跨仓库依赖

| 依赖 | 状态 | 对本计划的影响 |
|---|---|---|
| metamorphosis-core v0.1.29 | ✅ 已发布（git: c2j/metamorphosis） | RewriteEngine + RewriteContext 核心 API |
| metamorphosis-qed v0.1.29 | ✅ 已发布 | verify_rewrite + RichSchema + embedded Z3 |
| metamorphosis-verieql v0.1.0 | ✅ 已发布 | VeriEql::verify，零 metamorphosis 依赖 |
| metamorphosis-rules v0.1.29 | ✅ 已发布 | builtin_rules() → 14 条内置规则 |
| metamorphosis submodule | ✅ `lib/metamorphosis/` | **只读**，不可修改；如有特性需求向 metamorphosis 提 issue |
| ogsql-parser | ✅ ogexplain-core 传递依赖 | Parser::parse_sql, Statement, SchemaMap, SqlFormatter |

### 0.2 DiagnosticHint 兼容性（前置验证）

| 字段 | ogexplain-core | metamorphosis-core | 兼容 |
|------|---------------|-------------------|------|
| `rule_id: String` | ✅ | ✅ | ✅ |
| `table: Option<String>` | ✅ | ✅ | ✅ |
| `columns: Vec<String>` | ✅ | ✅ | ✅ |
| `severity: String` | ✅ | ✅ | ✅ |
| `detail: String` | ✅ | ✅ | ✅ |

**已知差异**：`metamorphosis_core::types::DiagnosticHint` 有 `#[non_exhaustive]`，外部 crate 不可 struct literal 构造。Phase 0 通过 serde 中转解决；后续向 metamorphosis 提 issue 添加 `DiagnosticHint::new()`。

### 0.3 范围

**In Scope：**
- 新建 `crates/ogexplain-optimizer/` library crate（无 binary）
- 迁移 `ogexplain-core/src/convergence.rs` → `optimizer/src/converge.rs`（删除 core 中模块）
- 迁移 `ogexplain-cli/src/optimize/mapper.rs` → `optimizer/src/mapper.rs`
- 新增 `optimizer/src/rewrite.rs`（SQL↔AST 封装层）
- 重写 `optimizer/src/verify.rs`（子进程 → library API）
- 迁移 `ogexplain-cli/src/optimize/mod.rs` → `optimizer/src/orchestrator.rs`（DB 注入重构）
- CLI `ogexplain optimize` 精简化（thin wrapper）
- 测试更新：import 路径修正 + orchestrator mock 测试

**Out of Scope（后续工作）：**
- metamorphosis 源码修改（只读子模块）
- `ogexplain-mcp` 集成 optimizer
- 向 metamorphosis 提交的 issue（DiagnosticHint::new 等）

### 0.4 约束

- 子模块 `lib/metamorphosis` 不可修改；有特性需求向 metamorphosis 提 issue
- 依赖走 git tag URL，不走本地 `lib/` 路径
- CLI 参数 `--skip-verify`, `--verify-engine`, `--max-iterations` 不变
- `ogexplain-optimizer` 为纯 library crate
- 所有 pub 项需 doc comment
- `ogexplain-cli` 不再直接依赖任何 metamorphosis crate

### 0.5 架构

```
ogexplain-analyzer workspace/
├── ogexplain-core              ← 诊断引擎（移除 convergence 模块）
├── ogexplain-optimizer  [NEW]  ← 闭环编排 library
│   ├── src/
│   │   ├── lib.rs              # pub mod {rewrite, converge, mapper, verify, orchestrator}
│   │   ├── rewrite.rs          # SQL↔AST 封装层（新增）
│   │   ├── converge.rs         # 收敛检测（从 core 迁移）
│   │   ├── mapper.rs           # 诊断→规则映射（从 CLI 迁移）
│   │   ├── verify.rs           # 验证集成（重写：子进程→库 API）
│   │   └── orchestrator.rs     # 核心循环（从 CLI 迁移，DB 注入）
│   └── Cargo.toml
│       deps: ogexplain-core, metamorphosis-core, metamorphosis-qed,
│             metamorphosis-verieql, metamorphosis-rules, ogsql-parser
│
├── ogexplain-cli               ← CLI 入口
│   └── src/
│       └── lib.rs              # optimize handler 委托 optimizer
│       deps: ogexplain-optimizer（移除直接 metamorphosis 依赖）
│       remove: src/optimize/mapper.rs, verify.rs, mod.rs
│
├── ogexplain-tui
├── ogexplain-mcp
└── ogsql-complexity
```

**依赖方向**：
```
ogexplain-cli → ogexplain-optimizer → { ogexplain-core, metamorphosis-* }
                                      ↘ ogsql-parser (transitive)
```

### 0.6 依赖声明

```toml
# crates/ogexplain-optimizer/Cargo.toml
[dependencies]
ogexplain-core = { path = "../ogexplain-core" }
metamorphosis-core = { git = "https://github.com/c2j/metamorphosis.git", tag = "v0.1.29", package = "metamorphosis-core" }
metamorphosis-qed = { git = "https://github.com/c2j/metamorphosis.git", tag = "v0.1.29", package = "metamorphosis-qed" }
metamorphosis-verieql = { git = "https://github.com/c2j/metamorphosis.git", tag = "v0.1.0", package = "metamorphosis-verieql" }
metamorphosis-rules = { git = "https://github.com/c2j/metamorphosis.git", tag = "v0.1.29", package = "metamorphosis-rules" }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "1"
tracing = "0.1"
```

### 0.7 核心 API 对照

| 步骤 | 当前（子进程） | 目标（库 API） |
|------|--------------|---------------|
| Rewrite | `Command::new("metamorphosis").arg("rewrite")` → 解析 stdout | `RewriteEngine::rewrite(ctx, stmts)` + SQL↔AST 封装 |
| Verify (QED) | `Command::new("metamorphosis").arg("verify")` → 解析 JSON | `verify_rewrite(rule_id, &ast1, &ast2, &schema, &config)` |
| Verify (VeriEQL) | 同上 | `VeriEql::verify(sql1, sql2, schema, constraints, bound, semantics)` |

---

## 1. 验证策略

### 1.1 单元测试目标

| 模块 | 最低测试数 | 覆盖场景 |
|------|-----------|---------|
| `converge.rs` | 7 | 5 种 StopReason + Continue + FixedPoint（保留现有） |
| `mapper.rs` | 9 | 映射表全分支 + filter_rewritable 质量门槛（保留现有） |
| `verify.rs` | 10+ | QED/VeriEQL status 解析 + schema 转换 + engine display roundtrip + decision 策略（保留现有，减少子进程测试） |
| `rewrite.rs` | 3+ | SQL parse→rewrite→format 完整路径（新增） |
| `orchestrator.rs` | 6+ | Mock ExplainExecutor 驱动 6 种收敛场景（新增） |

### 1.2 集成测试目标

| 文件 | 测试数 | 变更 |
|------|-------|------|
| `tests/optimize_e2e.rs` | 8 | import 路径更新，逻辑不变 |
| `tests/verify_e2e.rs` | 7 | import 路径更新，`#[cfg(feature = "db")]` gate 保持 |

### 1.3 回归验证

- `cargo test --workspace` 全通过（317 → ≥340）
- `cargo clippy --workspace` 零 warning
- `cargo fmt --all -- --check` 通过
- `ogexplain analyze` 功能不受影响
- `ogexplain mcp` 功能不受影响

---

## 2. 任务分解

### Phase 0 — 前置验证 + Skeleton

#### Task 0.1: DiagnosticHint 兼容性验证

**文件**: `crates/ogexplain-core/src/diagnostic_hint.rs`（不修改，仅验证）

- [ ] 确认 `ogexplain_core::DiagnosticHint` ↔ `metamorphosis_core::types::DiagnosticHint` 字段完全一致
- [ ] 编写 serde 中转转换函数（先不落地，Phase 2 使用）

**验收**: 手动确认字段映射表无误（6 字段已在上文 0.2 中确认）。

#### Task 0.2: 创建 `ogexplain-optimizer` 骨架

**文件**:
- [ ] `crates/ogexplain-optimizer/Cargo.toml` — package metadata + 依赖
- [ ] `crates/ogexplain-optimizer/src/lib.rs` — `pub mod {converge, mapper, verify, rewrite, orchestrator};`
- [ ] `crates/ogexplain-optimizer/src/converge.rs` — placeholder（下阶段迁移）
- [ ] `crates/ogexplain-optimizer/src/mapper.rs` — placeholder
- [ ] `crates/ogexplain-optimizer/src/verify.rs` — placeholder
- [ ] `crates/ogexplain-optimizer/src/rewrite.rs` — placeholder
- [ ] `crates/ogexplain-optimizer/src/orchestrator.rs` — placeholder

**改动**:
- [ ] `Cargo.toml`（workspace root）— `members` 添加 `"crates/ogexplain-optimizer"`

**验收**: `cargo build -p ogexplain-optimizer` 成功（首次编译含 Z3，可能较慢）。

---

### Phase 1 — Converge + Mapper 迁移

#### Task 1.1: 迁移 `converge.rs`

**源**: `crates/ogexplain-core/src/convergence.rs` (245 lines)
**目标**: `crates/ogexplain-optimizer/src/converge.rs`

- [ ] 复制全部类型和函数：`MetricsSnapshot`, `LoopConfig`, `LoopDecision`, `StopReason`, `should_continue()`
- [ ] 保留 `from_summary(&SummaryRow)` 方法（optimizer 依赖 core，方向正确）
- [ ] 复制全部 7 个单元测试
- [ ] 从 `ogexplain-core/src/lib.rs` 删除 `pub mod convergence;`
- [ ] 删除 `crates/ogexplain-core/src/convergence.rs`

**⚠️ 破坏性变更**：所有 `use ogexplain_core::convergence::...` 需要更新为 `use ogexplain_optimizer::converge::...`（在 Phase 5 批量处理）。

**验收**: `cargo test -p ogexplain-optimizer -- converge` 7/7 通过。

#### Task 1.2: 迁移 `mapper.rs`

**源**: `crates/ogexplain-cli/src/optimize/mapper.rs` (166 lines)
**目标**: `crates/ogexplain-optimizer/src/mapper.rs`

- [ ] 复制全部类型和函数：`RemediationAction`, `map_diagnostic()`, `filter_rewritable()`, `finding_to_hint()`
- [ ] 依赖保持不变（`ogexplain_core::analyzer::report::Finding`, `ogexplain_core::DiagnosticHint`）
- [ ] 复制全部 9 个单元测试

**验收**: `cargo test -p ogexplain-optimizer -- mapper` 9/9 通过。

---

### Phase 2 — Verify 重写（子进程 → 库 API）

#### Task 2.1: 重写 `verify.rs` 核心

**目标**: `crates/ogexplain-optimizer/src/verify.rs`

保留的类型系统（不变）：
- `VerifyEngine` (Qed / VeriEql) + Display + FromStr
- `VerifyStatus` (Equivalent / NotEquivalent / Unknown / Timeout / Skipped)
- `VerifyResult` (engine, status, elapsed_ms, original_sql, rewritten_sql, raw_output)
- `VerificationDecision` (Accept / Reject)
- `decide_verification_outcome()`
- `SkipReason`

删除的内容：
- `SchemaSource`（原用于区分 JSON/DDL schema 路径给子进程，不再需要）
- `VerifyError::SubprocessFailed`, `InvalidJson`, `MissingResultField`, `TimeoutElapsed`, `Io`
- `call_metamorphosis_verify()` — subprocess 完整替换
- `parse_metamorphosis_json()` — stdout JSON 解析不再需要
- `extract_counterexample()` — 不再需要手工解析
- 临时文件逻辑（PID/tid tag, tempfile）

新增的内容：

- [ ] `DiagnosticHint` 转换：`impl From<ogexplain_core::DiagnosticHint> for metamorphosis_core::types::DiagnosticHint`
- [ ] `verify_qed(original_sql, rewritten_sql, schema: &RichSchema, timeout_secs) -> Result<VerifyResult, VerifyError>`
  - 内部：`parse_sql → verify_rewrite("optimize", &ast1, &ast2, schema, &config)`
  - 映射 `ProofResult` → `VerifyStatus`
- [ ] `verify_verieql(original_sql, rewritten_sql, schema: &[TableSchema], constraints, bound) -> Result<VerifyResult, VerifyError>`
  - 内部：`VeriEql::verify(sql1, sql2, schema, constraints, Bound(bound), Semantics::Bag)`
  - 映射 `verieql_proof_result` → `VerifyStatus`
- [ ] `rich_schema_to_verieql(schema: &RichSchema) -> Vec<TableSchema>` — schema 格式转换
- [ ] `format_counterexample(counterexample: &Counterexample) -> String` — VeriEQL CE 格式化
- [ ] 编译期 parse helper：`parse_single(sql: &str) -> Result<Statement, VerifyError>`
- [ ] 新 `VerifyError` enum（精简为 Translation / Prover / Verification / Parse / Schema）

保留的子进程能力（过渡期）：
- [ ] `call_metamorphosis_verify()` 保留但标记 `#[deprecated]`，内部改为委托 `verify_qed()` / `verify_verieql()`
  - Phase 3 的 orchestrator 切换完成后，Phase 4 再删除 deprecated 函数

单元测试：
- [ ] engine display roundtrip（qed ↔ verieql）
- [ ] QED Equivalent / NotEquivalent / Unknown / Timeout 从 proof 结果映射
- [ ] VeriEQL Equivalent / NotEquivalent 映射
- [ ] schema 转换 rich_schema → verieql
- [ ] decision 策略（Accept / Reject）
- [ ] DiagnosticHint 转换无损
- [ ] SkipReason 不变
- [ ] 移除原有 7 个子进程相关测试（parse_metamorphosis_json, schema_missing 等）

**验收**: `cargo test -p ogexplain-optimizer -- verify` ≥10 通过。

#### Task 2.2: Schema 加载工具

**目标**: `crates/ogexplain-optimizer/src/rewrite.rs` 中添加 `load_schemas()` 辅助函数

- [ ] `load_schema_map(json_path: Option<&str>, sql_dir: Option<&str>) -> Result<Option<SchemaMap>, SchemaError>`
  - JSON: `serde_json::from_str` → `SchemaMap`
  - DDL dir: `metamorphosis_core::extractor::extract_schema_from_dir`
- [ ] `load_rich_schema(json_path: Option<&str>, sql_dir: Option<&str>) -> Result<Option<RichSchema>, SchemaError>`
  - JSON: `serde_json::from_str` → `HashMap<String, TableSchemaEntry>` → `schema_entries_to_ddl` → parse → `extract_rich_schema`
  - DDL dir: parse DDL → `extract_rich_schema`
- [ ] `SchemaError` enum（JsonParse / DirNotFound / NoSqlFiles / AllSkipped）

**验收**: 单元测试覆盖 JSON 和 DDL 两种 schema 加载路径。

---

### Phase 3 — Rewrite 封装 + Orchestrator DB 注入

#### Task 3.1: Rewrite 封装层

**目标**: `crates/ogexplain-optimizer/src/rewrite.rs`

- [ ] `rewrite_sql(sql, schema, hints, rules) -> Result<Option<String>, RewriteError>`
  - parse SQL → AST
  - build `RuleRegistry` from `metamorphosis_rules::builtin_rules()` filtered by `rules`
  - build `RewriteContext` with schema + config + hints
  - call `RewriteEngine::rewrite(ctx, stmts)`
  - format AST → SQL string
  - return `None` if no change
- [ ] `RewriteError` enum（Parse / NoRulesMatched / Format）

**依赖**：需要确认 `metamorphosis-rules` crate 的 `builtin_rules()` 确认为 pub fn 且可用。

**验收**: 单元测试 3 个场景 — rewrite applied, no change, parse error。

#### Task 3.2: Orchestrator 迁移 + DB 注入

**源**: `crates/ogexplain-cli/src/optimize/mod.rs` (582 lines)
**目标**: `crates/ogexplain-optimizer/src/orchestrator.rs`

改动点（相对于当前 mod.rs）：

- [ ] **DB 连接注入**: 定义 `ExplainExecutor` trait
  ```rust
  pub trait ExplainExecutor {
      fn fetch_explain(&self, sql: &str, analyze: bool) -> Result<String, ExplainError>;
  }
  ```
- [ ] **Rewrite 步骤**: `call_metamorphosis_rewrite()` → `crate::rewrite::rewrite_sql()`
- [ ] **Verify 步骤**: `call_metamorphosis_verify()` → `crate::verify::verify_qed()` / `verify_verieql()`
- [ ] **Metamorphosis 可用性检查**: 移除 `check_metamorphosis_available()`（不再需要子进程探测）
- [ ] **OptimizeArgs → OptimizeConfig**: 移除 CLI 特有字段（`metamorphosis_path`, `config_path`, `name`），DB 连接细节由 `ExplainExecutor` 封装
- [ ] **finalize() + render_report()**: 保留，但移到 optimizer crate 内部
- [ ] **hash_sql() + update_plateau()**: 保留不变
- [ ] **主循环逻辑**: 不变（EXPLAIN → diagnose → map → rewrite → verify → converge）
- [ ] 保留全部 5 个现有单元测试（hash_sql, plateau_count, render_report）

Mock 测试（新增）：
- [ ] 实现 `MockExecutor`，返回预制 EXPLAIN 文本
- [ ] 6 种收敛场景测试：Success / Regression / MaxIterations / Plateau / NoRewritable / FixedPoint / Continue

**验收**: `cargo test -p ogexplain-optimizer -- orchestrator` ≥11 通过（5 保留 + 6 新增）。

---

### Phase 4 — CLI 精简化

#### Task 4.1: ogexplain-cli optimize handler 精简

**改动**:

- [ ] `crates/ogexplain-cli/Cargo.toml` — 添加 `ogexplain-optimizer` 依赖；确认无 metamorphosis 直接依赖
- [ ] `crates/ogexplain-cli/src/lib.rs` — optimize handler 改为委托 optimizer：
  ```rust
  // Before:
  optimize::run_optimize(optimize::OptimizeArgs { ... })
  
  // After:
  let executor = DbExecutor { config_path, name, verbose };
  ogexplain_optimizer::orchestrator::run_optimize(
      ogexplain_optimizer::orchestrator::OptimizeConfig { ... },
      &executor,
  )
  ```
  添加 `DbExecutor` 实现 `ExplainExecutor` trait（封装现有 `crate::db::fetch_explain`）

- [ ] 删除文件：
  - `crates/ogexplain-cli/src/optimize/mapper.rs`
  - `crates/ogexplain-cli/src/optimize/verify.rs`
  - `crates/ogexplain-cli/src/optimize/mod.rs`
- [ ] 删除 `pub mod optimize;` → 改为 inline `mod optimize_handler`（仅含 DbExecutor + clap arg 解析）

**验收**:
- [ ] `cargo build -p ogexplain-cli` 成功
- [ ] `ogexplain optimize --help` 输出与重构前 diff 为空
- [ ] `ogexplain analyze` 正常
- [ ] `ogexplain mcp` 正常

---

### Phase 5 — 测试更新 + 回归验证

#### Task 5.1: 集成测试 import 更新

**文件**: `tests/optimize_e2e.rs`

- [ ] `use ogexplain_cli::optimize::mapper::...` → `use ogexplain_optimizer::mapper::...`
- [ ] `use ogexplain_core::convergence::...` → `use ogexplain_optimizer::converge::...`
- [ ] 8 个测试逻辑不变

**文件**: `tests/verify_e2e.rs`

- [ ] `use ogexplain_cli::optimize::verify::...` → `use ogexplain_optimizer::verify::...`
- [ ] 注意：当前测试 `#[cfg(feature = "db")]` gated 且依赖 metamorphosis 子进程 — Phase 2 后可能需要调整（库 API 不再需要子进程），但如果 metamorphosis-qed 的 Z3 solver 可用，可以去掉子进程依赖
- [ ] 评估是否可以将 `#[cfg(feature = "db")]` 改为始终编译（仅依赖 Z3）

#### Task 5.2: 全量回归

- [ ] `cargo test --workspace` 全通过
- [ ] `cargo clippy --workspace` 零 warning
- [ ] `cargo fmt --all -- --check` 通过
- [ ] `cargo build --workspace` 成功

---

## 3. 文件清单

### 新建文件

| # | 文件路径 | Phase | 估算行数 |
|---|---------|-------|---------|
| 1 | `crates/ogexplain-optimizer/Cargo.toml` | 0 | 30 |
| 2 | `crates/ogexplain-optimizer/src/lib.rs` | 0 | 10 |
| 3 | `crates/ogexplain-optimizer/src/converge.rs` | 1 | 250 |
| 4 | `crates/ogexplain-optimizer/src/mapper.rs` | 1 | 170 |
| 5 | `crates/ogexplain-optimizer/src/verify.rs` | 2 | 450 |
| 6 | `crates/ogexplain-optimizer/src/rewrite.rs` | 3 | 200 |
| 7 | `crates/ogexplain-optimizer/src/orchestrator.rs` | 3 | 500 |

### 修改文件

| # | 文件路径 | Phase | 变更 |
|---|---------|-------|------|
| 8 | `Cargo.toml` (workspace) | 0 | members 添加 optimizer |
| 9 | `crates/ogexplain-core/src/lib.rs` | 1 | 删除 `pub mod convergence;` |
| 10 | `crates/ogexplain-cli/Cargo.toml` | 4 | 添加 `ogexplain-optimizer` dep |
| 11 | `crates/ogexplain-cli/src/lib.rs` | 4 | optimize handler 改为委托 |
| 12 | `tests/optimize_e2e.rs` | 5 | import 更新 |
| 13 | `tests/verify_e2e.rs` | 5 | import 更新 |

### 删除文件

| # | 文件路径 | Phase | 原因 |
|---|---------|-------|------|
| 14 | `crates/ogexplain-core/src/convergence.rs` | 1 | 迁移至 optimizer |
| 15 | `crates/ogexplain-cli/src/optimize/mapper.rs` | 4 | 迁移至 optimizer |
| 16 | `crates/ogexplain-cli/src/optimize/verify.rs` | 4 | 迁移至 optimizer |
| 17 | `crates/ogexplain-cli/src/optimize/mod.rs` | 4 | 核心逻辑迁移至 optimizer |

---

## 4. 风险与缓解

| 风险 | 概率 | 影响 | 缓解 |
|------|------|------|------|
| Z3 embedded 首次编译需 10-20min | 高 | CI 慢 | Phase 0 先验证；CI 缓存 target/ |
| `#[non_exhaustive]` 阻止 DiagnosticHint 构造 | 已确认 | Phase 2 受阻 | Phase 0 serde 中转方案；后续向 metamorphosis 提 issue |
| metamorphosis-rules `builtin_rules()` 非 pub | 低 | Phase 3 受阻 | Phase 0 验证 API 可用性 |
| `metamorphosis-rules` crate 名不确定 | 低 | Phase 3 受阻 | Phase 0 确认 crate name |
| `ogexplain analyze` / `ogexplain mcp` 回归 | 中 | 用户体验 | Phase 5 全量测试 |
| git tag 不可用 | 低 | 编译失败 | 回退到 branch 或 commit hash |

---

## 5. 验收标准（最终）

| # | 验收项 | 验证方法 |
|---|--------|---------|
| V1 | `cargo build -p ogexplain-optimizer` 成功 | 编译通过 |
| V2 | `cargo build -p ogexplain-cli` 成功 | ogexplain-cli 不直接依赖 metamorphosis |
| V3 | `ogexplain optimize --help` 输出不变 | diff 对比 |
| V4 | `cargo test -p ogexplain-optimizer` 全通过 | converge(7) + mapper(9) + verify(10+) + rewrite(3+) + orchestrator(11+) |
| V5 | `cargo test --test optimize_e2e` 全通过（8/8） | import 更新后功能不变 |
| V6 | `cargo test --test verify_e2e` 编译通过 | gate 逻辑保持 |
| V7 | `ogexplain analyze` 功能不受影响 | 相关测试全通过 |
| V8 | `ogexplain mcp` 功能不受影响 | MCP 测试通过 |
| V9 | `cargo test --workspace` 全通过 | 零回归 |
| V10 | `cargo clippy --workspace` 零 warning | 代码质量 |
