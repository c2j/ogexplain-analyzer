# ogexplain-analyzer 实施方案

> **版本**: v1.0
> **日期**: 2026-04-11
> **状态**: Approved
> **设计规格**: `.sisyphus/plans/ogexplain-analyzer-spec.md`

---

## 1. 技术选型

### 1.1 语言与运行时

| 层 | 选型 | 版本 | 理由 |
|---|------|------|------|
| 语言 | Rust | 2021 edition | enum 完美匹配 80+ 节点类型，单二进制分发，零开销抽象 |
| TUI 框架 | `ratatui` | 0.30 | 官方默认 crossterm 后端，跨平台，生态最丰富 |
| 树控件 | `tui-tree-widget` | 0.24 | 575K+ 下载，内置展开/折叠/选择/滚动 |
| 文本输入 | `ratatui-textarea` | 0.8 | ratatui 官方组织维护，多行编辑、粘贴、撤销 |
| 异步 | `tokio` | 1 | Component 间 Action 派发，未来分析可异步 |
| 快照测试 | `insta` (YAML) | 1 | fixture 驱动，`assert_yaml_snapshot!` 自动回归 |
| CLI | `clap` | 4 (derive) | 成熟、功能完整 |
| 错误处理 | `anyhow` + `thiserror` | — | 应用级 + 库级 |

### 1.2 TUI 架构模式

采用 **Elm Architecture (TEA)** — Model-View-Update：

```
Event (crossterm) → handle_events() → Option<Action>
    ↓
action_tx.send(Action) → action_rx.recv()
    ↓
update(Action) → mutates Model
    ↓
draw(Frame, Rect) → renders UI
```

Phase 1 使用同步 `loop { event → update → draw }`。若分析变慢再抽成 `tokio::spawn` + channel。

### 1.3 Crate 组织（Cargo workspace）

```
ogexplain-analyzer/                    # workspace root
├── Cargo.toml                         # [workspace]
├── crates/
│   ├── ogexplain-core/                # 核心库（parser + model + analyzer + suggester）
│   ├── ogexplain-cli/                 # CLI 前端
│   └── ogexplain-tui/                 # TUI 前端
├── tests/fixtures/                    # EXPLAIN TEXT 样例（三 crate 共享）
├── .sisyphus/plans/                   # 设计规格 + 本文件
├── GaussDB-2.23.07.210/              # 参考文档
└── lib/openGauss-server/             # openGauss 源码（gitignored submodule）
```

CLI 和 TUI 共享 `ogexplain-core`，各自编译为独立二进制。

---

## 2. 依赖清单

### `crates/ogexplain-core/Cargo.toml`

```toml
[package]
name = "ogexplain-core"
version = "0.1.0"
edition = "2021"

[dependencies]
regex = "1"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "2"
toml = "0.8"

[dev-dependencies]
insta = { version = "1", features = ["yaml", "filters"] }
```

### `crates/ogexplain-cli/Cargo.toml`

```toml
[package]
name = "ogexplain-cli"
version = "0.1.0"
edition = "2021"

[[bin]]
name = "ogexplain"
path = "src/main.rs"

[dependencies]
ogexplain-core = { path = "../ogexplain-core" }
clap = { version = "4", features = ["derive"] }
colored = "3"
anyhow = "1"
serde_json = "1"
```

### `crates/ogexplain-tui/Cargo.toml`

```toml
[package]
name = "ogexplain-tui"
version = "0.1.0"
edition = "2021"

[[bin]]
name = "ogexplain-tui"
path = "src/main.rs"

[dependencies]
ogexplain-core = { path = "../ogexplain-core" }
ratatui = "0.30"
crossterm = "0.29"
tui-tree-widget = "0.24"
ratatui-textarea = "0.8"
tokio = { version = "1", features = ["full"] }
color-eyre = "0.6"
clap = { version = "4", features = ["derive"] }
```

### workspace root `Cargo.toml`

```toml
[workspace]
members = [
    "crates/ogexplain-core",
    "crates/ogexplain-cli",
    "crates/ogexplain-tui",
]
resolver = "2"
```

---

## 3. 核心库目录结构

```
crates/ogexplain-core/src/
├── lib.rs                        # 公共 API：parse(), analyze(), suggest()
│
├── parser/                       # 解析层
│   ├── mod.rs                    # pub fn parse(text: &str) -> Result<ExplainPlan>
│   ├── line_classifier.rs        # 逐行分类：NodeLine / PropertyLine / SummaryLine / ...
│   └── tree_builder.rs           # 缩进栈 → PlanNode 树
│
├── model/                        # 数据模型（全部 #[derive(Serialize)]）
│   ├── mod.rs
│   ├── plan.rs                   # ExplainPlan, PlanSummary
│   ├── node_type.rs              # NodeType enum (80+ variants), NodeTypeCategory
│   ├── join_type.rs              # JoinType enum
│   ├── streaming.rs              # StreamingType enum
│   ├── cost.rs                   # EstimatedCost, ActualStats
│   └── buffer.rs                 # BufferStats, NodeProperty
│
├── analyzer/                     # 分析层
│   ├── mod.rs                    # DiagnosticEngine::analyze(plan) -> DiagnosticReport
│   ├── context.rs                # PlanContext, GlobalStats
│   ├── config.rs                 # DiagnosticConfig (阈值、启停规则)
│   ├── report.rs                 # DiagnosticReport, Finding, Severity
│   └── rules/                    # 规则实现
│       ├── mod.rs                # DiagnosticRule trait + 注册
│       ├── scan_rules.rs         # SCAN-001 ~ SCAN-005
│       ├── join_rules.rs         # JOIN-001 ~ JOIN-005
│       ├── memory_rules.rs       # MEM-001 ~ MEM-004
│       ├── sort_rules.rs         # SORT-001 ~ SORT-003
│       ├── network_rules.rs      # NET-001 ~ NET-004
│       ├── estimation_rules.rs   # EST-001 ~ EST-004
│       ├── general_rules.rs      # GEN-001 ~ GEN-004
│       ├── pushdown_rules.rs     # PUSH-001 ~ PUSH-006
│       ├── type_coercion_rules.rs # TYPE-001 ~ TYPE-005
│       ├── vectorization_rules.rs # VEC-001 ~ VEC-005
│       ├── subquery_rules.rs     # SUBQ-001 ~ SUBQ-005
│       ├── distribution_rules.rs # DIST-001 ~ DIST-005
│       └── storage_rules.rs      # STORE-001 ~ STORE-004
│
└── suggester/                    # 建议引擎
    ├── mod.rs                    # SuggestionEngine::suggest(findings) -> Vec<Suggestion>
    ├── suggestion.rs             # Suggestion, Action, SuggestionCategory
    ├── mapper.rs                 # 规则 → 建议映射表
    └── synthesizer.rs            # 6 种跨规则综合推理模式
```

### 核心 API 设计

```rust
// ogexplain-core/src/lib.rs

/// 解析 EXPLAIN TEXT 输出
pub fn parse(text: &str) -> Result<ExplainPlan>;

/// 分析执行计划，返回诊断报告
pub fn analyze(plan: &ExplainPlan) -> DiagnosticReport;

/// 分析（带自定义配置）
pub fn analyze_with_config(plan: &ExplainPlan, config: &DiagnosticConfig) -> DiagnosticReport;

/// 生成优化建议
pub fn suggest(report: &DiagnosticReport) -> Vec<Suggestion>;

/// 一步到位：解析 + 分析 + 建议
pub fn run(text: &str) -> Result<(ExplainPlan, DiagnosticReport, Vec<Suggestion>)>;

/// 一步到位（带配置）
pub fn run_with_config(text: &str, config: &DiagnosticConfig) -> Result<(ExplainPlan, DiagnosticReport, Vec<Suggestion>)>;
```

---

## 4. TUI 详细设计

### 4.1 UI 布局

```
┌─────────────────────────────────────────────────────────────┐
│ ogexplain-analyzer                              [q]uit [?]help│
├──────────────────────┬──────────────────────────────────────┤
│ Plan Tree            │ Node Detail                          │
│ ─────────            │ ────────────                         │
│ ▼ Hash Join    [🟡]  │ Type: Hash Join                      │
│   ▸ Seq Scan (t1)[🔴]│ Cost: 12.34..523.45                  │
│   ▸ Hash             │ Actual: 23.456ms                     │
│     ▸ Seq Scan (t2)  │ Rows: 5000 est / 50000 actual       │
│                      │                                      │
│ legend:              │ ── Diagnostics ──                    │
│ 🔴 Critical          │ 🔴 EST-001 行数严重低估              │
│ 🟡 Warning           │    实际 50000 vs 估算 500 (100x)    │
│ 🟢 Info              │    💡 ANALYZE t1;                     │
│                      │                                      │
├──────────────────────┴──────────────────────────────────────┤
│ :load <file>  or paste EXPLAIN here, then Ctrl+P to parse  │
│ > Seq Scan on t1  (cost=0.00..12.00 rows=100 width=4)       │
│ >   Filter: (status = 'active')                              │
├─────────────────────────────────────────────────────────────┤
│ ↑↓ navigate  Enter expand  Tab switch  Ctrl+P parse  q quit │
└─────────────────────────────────────────────────────────────┘
```

### 4.2 面板组成

| 面板 | 实现 | 功能 |
|------|------|------|
| `TreePanel` | `tui-tree-widget` | 可折叠计划树，节点前缀图标标记诊断严重度 |
| `DetailPanel` | ratatui `Paragraph` + `Table` | 选中节点的 Cost/Actual/Properties + 关联诊断 + 建议 |
| `InputPanel` | `ratatui-textarea` | 多行文本编辑，支持粘贴、`:load <file>` 命令 |
| `StatusBar` | ratatui `Paragraph` | 上下文相关快捷键提示 |

### 4.3 Action 定义

```rust
pub enum Action {
    // 解析
    ParseExplain,                    // Ctrl+P
    LoadFile(String),                // :load <path>

    // 树导航
    TreeUp, TreeDown,                // ↑ ↓
    TreeToggle,                      // Enter（展开/折叠）
    TreeExpandAll, TreeCollapseAll,  // Shift+Enter / Backspace

    // 面板焦点
    NextPanel, PrevPanel,            // Tab / Shift+Tab

    // 视图模式（DetailPanel 内）
    ToggleViewMode,                  // Ctrl+V：detail / suggestions / stats

    // 应用
    Quit,                            // q / Ctrl+C
    Resize(u16, u16),               // 终端大小变化
}
```

### 4.4 焦点状态机

```
AppMode::Input  (默认启动状态，焦点在 InputPanel)
  ↓ Ctrl+P（解析成功）
AppMode::Browse (焦点在 TreePanel)
  ↓ Tab          → TreePanel ↔ DetailPanel 循环
  ↓ Tab (回到 InputPanel) → AppMode::Input
```

### 4.5 树节点颜色映射

节点严重度取其自身及所有子节点的最高严重度：

```rust
fn node_severity(node: &PlanNode, findings: &[Finding]) -> Option<Severity> {
    let self_severity = findings.iter()
        .find(|f| f.node_line == node.line_number)
        .map(|f| f.severity.clone());
    let child_severity = node.children.iter()
        .filter_map(|c| node_severity(c, findings))
        .max();
    std::cmp::max(self_severity, child_severity)
}
```

TUI 渲染时：
- 🔴 Critical → `Color::Red`
- 🟡 Warning → `Color::Yellow`
- 🟢 Info → `Color::Green`
- 无诊断 → 默认前景色

### 4.6 TUI 目录结构

```
crates/ogexplain-tui/src/
├── main.rs                      # 入口：终端初始化、clap 参数、启动 App
├── app.rs                       # App struct（TEA Model）：持有所有状态
├── action.rs                    # Action enum
├── event.rs                     # crossterm 事件 → Action 映射
└── components/
    ├── mod.rs                   # Component trait
    ├── tree_panel.rs            # 计划树浏览
    ├── detail_panel.rs          # 节点详情 + 诊断结果
    ├── input_panel.rs           # EXPLAIN 文本输入
    └── status_bar.rs            # 底部快捷键提示
```

---

## 5. 测试策略

### 5.1 解析器测试（insta YAML snapshot）

```rust
// tests/parser_tests.rs (在 ogexplain-core 中)
use insta::assert_yaml_snapshot;

fn parse_fixture(name: &str) -> ogexplain_core::model::ExplainPlan {
    let input = include_str!(concat!("../fixtures/", name));
    ogexplain_core::parse(input).expect("fixture should parse")
}

#[test]
fn simple_seq_scan() {
    let plan = parse_fixture("01_simple_seq_scan.txt");
    insta::with_settings!({ sort_maps => true }, {
        assert_yaml_snapshot!(plan);
    });
}
```

### 5.2 规则测试（正面 + 负面）

每条规则至少 2 个测试：
- **正面**：应触发该规则的 EXPLAIN 输入 → 断言 finding 存在
- **负面**：不应触发该规则的 EXPLAIN 输入 → 断言 finding 不存在

### 5.3 Fixture 清单

| 文件名 | 覆盖场景 |
|--------|---------|
| `01_simple_seq_scan.txt` | 基本 Seq Scan |
| `02_index_scan_filter.txt` | Index Scan + Filter + Rows Removed |
| `03_hash_join.txt` | Hash Join + Hash Cond |
| `04_hash_join_spill.txt` | Hash 溢出 (Batches > 1) |
| `05_nested_loop_large.txt` | Nested Loop 大数据集 |
| `06_sort_external_merge.txt` | Sort 溢出到磁盘 |
| `07_multi_level_join.txt` | 多层 Join 嵌套 |
| `08_streaming_redistribute.txt` | 分布式 Streaming |
| `09_vector_plan.txt` | 向量化节点 |
| `10_cstore_scan.txt` | CStore 列存扫描 |
| `11_pretty_mode.txt` | Pretty 模式（节点 ID + `--` 前缀） |
| `12_pushback_dn_scan.txt` | FQS 下推 + Data Node Scan |
| `13_implicit_cast_pattern.txt` | Seq Scan + Filter 高 Rows Removed（疑似隐式转换） |
| `14_subplan.txt` | SubPlan / InitPlan |
| `15_partitioned_scan.txt` | Partitioned * 节点 |
| `16_merge_stmt.txt` | MERGE 语句 |
| `17_full_analyze.txt` | 完整 EXPLAIN ANALYZE（含 Buffers、I/O） |
| `18_distributed_multi_dn.txt` | 分布式多 DN（Max/Min Buffers、Sort Method range） |
| `complex_plan.txt` | 复杂综合场景 |

---

## 6. 实施阶段

### Phase 1 — 核心解析（~1 周）

**目标**：TEXT 格式输入 → 结构化 `ExplainPlan`

**交付物**：
- [ ] Workspace 初始化（`Cargo.toml` + 三个 crate 骨架）
- [ ] `ogexplain-core`：`model/` 全部数据类型（`#[derive(Serialize)]`）
- [ ] `ogexplain-core`：`parser/line_classifier.rs`（行分类器 + 正则）
- [ ] `ogexplain-core`：`parser/tree_builder.rs`（缩进栈树构建）
- [ ] `ogexplain-core`：`parser/mod.rs`（`pub fn parse()` 公共 API）
- [ ] 10+ fixture 文件覆盖核心节点类型
- [ ] 每个 fixture 对应的 `insta::assert_yaml_snapshot!` 测试
- [ ] `cargo test -p ogexplain-core` 全过

**验证**：
```bash
cargo test -p ogexplain-core           # snapshot 全 green
cargo insta review                      # 确认 snapshot 内容正确
```

### Phase 2 — 诊断引擎 + CLI（~1 周）

**目标**：解析结果 → 诊断规则 → 文本/JSON 报告

**交付物**：
- [ ] `ogexplain-core`：`analyzer/` 框架（`DiagnosticRule` trait + `DiagnosticEngine`）
- [ ] `ogexplain-core`：Phase 1 规则（15 条，见下表）
- [ ] `ogexplain-core`：`suggester/mapper.rs`（规则 → 建议映射）
- [ ] `ogexplain-cli`：基本 CLI（`ogexplain analyze file.txt -o text|json`）
- [ ] 每条规则正面 + 负面测试
- [ ] CLI 输出验证

**Phase 1 规则（15 条，覆盖最高价值场景）**：

| Rule ID | 名称 | 分类 |
|---------|------|------|
| SCAN-001 | 大表全表扫描 | scan_rules |
| SCAN-004 | 未使用索引的 Filter | scan_rules |
| JOIN-001 | Nested Loop 大数据集 | join_rules |
| JOIN-002 | Hash 溢出到磁盘 | join_rules |
| MEM-001 | Sort 溢出到磁盘 | memory_rules |
| MEM-004 | 峰值内存过高 | memory_rules |
| SORT-003 | 重复排序 | sort_rules |
| EST-001 | 行数严重低估 | estimation_rules |
| EST-004 | Nested Loop 因低估选中 | estimation_rules |
| PUSH-001 | 查询未下推 | pushdown_rules |
| PUSH-002 | 多层 Stream 嵌套 | pushdown_rules |
| TYPE-001 | 疑似隐式类型转换 | type_coercion_rules |
| TYPE-004 | LIKE 前缀通配符 | type_coercion_rules |
| VEC-001 | 混合向量化/行引擎 | vectorization_rules |
| NET-001 | 广播大表 | network_rules |

**验证**：
```bash
cargo test -p ogexplain-core                   # 全过
cargo run -p ogexplain-cli -- analyze tests/fixtures/complex_plan.txt
cargo run -p ogexplain-cli -- analyze tests/fixtures/complex_plan.txt -o json
```

### Phase 3 — TUI 骨架 + 交互（~1-2 周）

**目标**：可交互的计划树浏览器

**交付物**：
- [ ] `ogexplain-tui`：终端初始化/恢复（crossterm alternate screen）
- [ ] `ogexplain-tui`：`app.rs` 状态机（TEA Model）
- [ ] `ogexplain-tui`：`action.rs` + `event.rs`（事件 → Action）
- [ ] `ogexplain-tui`：`input_panel.rs`（TextArea，粘贴 + `:load` 命令）
- [ ] `ogexplain-tui`：`tree_panel.rs`（tui-tree-widget，展开/折叠/导航）
- [ ] `ogexplain-tui`：`detail_panel.rs`（节点详情 + 诊断 + 建议）
- [ ] `ogexplain-tui`：`status_bar.rs`（快捷键提示）
- [ ] 焦点状态机（Input → Browse 循环）
- [ ] 节点颜色编码（按最高严重度）
- [ ] 支持 `ogexplain-tui file.txt`（直接加载）和 `ogexplain-tui`（启动后粘贴）
- [ ] 所有快捷键工作：↑↓ / Enter / Tab / Ctrl+P / Ctrl+V / q

**验证**：
```bash
cargo run -p ogexplain-tui -- tests/fixtures/complex_plan.txt
# 手动验证：树展开/折叠、节点选择、详情切换、快捷键
```

### Phase 4 — 完善 + 打磨（~1 周）

**交付物**：
- [ ] 剩余诊断规则（全部 45+ 覆盖）
- [ ] 建议引擎 6 种跨规则综合推理模式
- [ ] `suggester/synthesizer.rs` 完整实现
- [ ] Markdown 输出（CLI `-o markdown`）
- [ ] TOML 配置文件支持（`--config file.toml`）
- [ ] 更多 fixture 覆盖边界情况
- [ ] 错误处理优化（友好错误信息、parse error 定位到行号）
- [ ] 性能优化（大 EXPLAIN 输出 > 1000 行的解析性能）

**验证**：
```bash
cargo test --workspace                     # 全部测试通过
cargo run -p ogexplain-cli -- --help       # CLI 帮助完整
cargo run -p ogexplain-tui -- --help       # TUI 帮助完整
```

---

## 7. 关键架构约束

1. **core 是纯 library** — `ogexplain-core` 不依赖任何 UI/IO crate（no ratatui, no crossterm, no clap）。CLI 和 TUI 作为 frontend 消费 core 的公共 API。
2. **model 全部 Serialize** — 所有数据结构 `#[derive(Serialize)]`，为 insta YAML snapshot 和未来 JSON API 铺路。
3. **先 CLI 验证，再 TUI** — Phase 1-2 用 `cargo test` + CLI 快速验证正确性。TUI 在 Phase 3 进入，不阻塞核心逻辑。
4. **parser 不 panic** — 解析器对所有未知节点类型回退到 `NodeType::Unknown(String)`，不中断解析。错误通过 `Result` 传播。
5. **诊断规则可独立测试** — 每条规则有独立的 positive/negative fixture，不依赖其他规则。
6. **TUI 状态与渲染分离** — App state（TEA Model）只管数据和逻辑，`draw()` 只负责从 state 读取并渲染。不反向修改。

---

## 8. 开发命令速查

```bash
# 构建
cargo build                              # 全 workspace
cargo build -p ogexplain-core            # 仅核心库
cargo build -p ogexplain-cli             # 仅 CLI
cargo build -p ogexplain-tui             # 仅 TUI

# 测试
cargo test                               # 全 workspace
cargo test -p ogexplain-core             # 核心库测试
cargo test -p ogexplain-core -- test_parser  # 仅 parser 测试
cargo insta review                       # 交互式 review pending snapshots

# 运行
cargo run -p ogexplain-cli -- analyze tests/fixtures/complex_plan.txt
cargo run -p ogexplain-cli -- analyze tests/fixtures/complex_plan.txt -o json
cargo run -p ogexplain-tui -- tests/fixtures/complex_plan.txt
cargo run -p ogexplain-tui               # 启动后粘贴模式

# Lint
cargo fmt --check                        # 格式检查
cargo clippy --workspace                 # lint
```
