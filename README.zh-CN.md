# ogexplain-analyzer

[English](README.md) | [中文](README.zh-CN.md)

OpenGauss/GaussDB `EXPLAIN` / `EXPLAIN ANALYZE` 输出解析与性能诊断工具。解析 TEXT 格式执行计划，运行 25 条诊断规则（包含下推、向量化、流式计算、隐式类型转换等 OpenGauss 专项检查），输出诊断发现与参数化优化建议。

## 功能特性

- **完整的 EXPLAIN TEXT 解析** — 支持 `EXPLAIN` 和 `EXPLAIN ANALYZE` 输出，包括 pretty 模式（`N --` 前缀）、向量化节点、CStore 扫描、Streaming 算子以及 OG 专属属性。
- **25 条诊断规则** — 覆盖扫描、连接、内存、排序、网络、估算偏差、下推、类型转换、向量化、子查询、聚合、分布、统计信息、分区和执行计划通用健康检查。
- **参数化建议** — 规则从计划属性中提取表名、列名和具体数值，生成可操作建议（如 `CREATE INDEX ON orders(status)`）。跨规则综合分析将多个发现映射为高层建议。
- **SQL 复杂度评分** — 集成 `ogsql-complexity` crate，按 0–100 分制评估 SQL 语句复杂度，支持 GaussDB 四维模型（SQL 结构、PL 逻辑、高级特性、扩展功能）。
- **国际化支持** — 通过 `--lang` 参数或系统语言自动检测，支持英文和中文（`zh-CN`）输出。
- **多种接口** — CLI 用于脚本集成，TUI 用于交互式探索，库 crate 用于嵌入式调用。
- **数据库直连 EXPLAIN** — `explain` 子命令直接连接 OpenGauss/GaussDB，执行 `EXPLAIN [ANALYZE]` 并一步完成分析。
- **批量处理** — 解析包含 SQL 和 EXPLAIN 块混合的多语句文件，支持导出汇总表为 CSV。

## 快速开始

```bash
# 构建
cargo build --workspace

# 分析 EXPLAIN 输出文件（文本报告）
cargo run -p ogexplain-cli -- analyze tests/fixtures/03_hash_join.txt

# JSON 输出
cargo run -p ogexplain-cli -- analyze tests/fixtures/03_hash_join.txt -o json

# 从标准输入读取
cat tests/fixtures/01_simple_seq_scan.txt | cargo run -p ogexplain-cli -- analyze -

# 启动 TUI 加载文件
cargo run -p ogexplain-tui -- tests/fixtures/10_complex_plan.txt

# 启动 TUI 粘贴模式（Ctrl+P 解析）
cargo run -p ogexplain-tui
```

## 安装

从源码构建：

```bash
git clone https://github.com/c2j/ogexplain-analyzer.git
cd ogexplain-analyzer
cargo build --release

# CLI 二进制
./target/release/ogexplain analyze file.txt

# TUI 二进制
./target/release/ogexplain-tui file.txt
```

## 使用方法

### CLI

```bash
ogexplain analyze <file> [options]
```

| 参数 | 默认值 | 说明 |
|------|--------|------|
| `-o, --output` | `text` | 输出格式：`text` 或 `json` |
| `--threshold` | `info` | 最低严重级别：`critical`、`warning`、`info` |
| `-q, --quiet` | — | 仅显示诊断结果，不显示执行计划树 |
| `-v, --verbose` | — | 详细输出 |
| `--multi` | — | 启用多块解析 |
| `--csv <path>` | — | 导出汇总表为 CSV（使用 `-` 输出到 stdout） |
| `--lang` | `auto` | 语言：`en`、`zh-CN` 或 `auto`（跟随系统语言） |

#### 数据库直连 EXPLAIN（需要 `db` feature）

```bash
# 构建（启用数据库支持）
cargo build -p ogexplain-cli --features db

# 对远程数据库执行 EXPLAIN
ogexplain explain -d "host=... port=5432 dbname=mydb user=gaussdb password=... sslmode=disable" -s "SELECT * FROM orders WHERE status = 'pending'"

# 执行 EXPLAIN ANALYZE（会实际执行查询）
ogexplain explain -d "host=..." -s "SELECT ..." --analyze

# 从文件读取 SQL
ogexplain explain -d "host=..." -f query.sql
```

### TUI

```bash
ogexplain-tui [file]
```

| 按键 | 操作 |
|------|------|
| `Ctrl+P` | 解析粘贴的 EXPLAIN 文本 |
| `Tab` | 在面板间切换焦点 |
| `↑` `↓` | 导航执行计划树 |
| `Enter` | 展开 / 折叠节点 |
| `q` | 退出 |

### 库调用

```rust
use ogexplain_core::{parse, analyze, analyze_with_config};
use ogexplain_core::analyzer::config::DiagnosticConfig;

// 解析 EXPLAIN 文本
let plan = parse(&explain_text)?;

// 使用默认配置分析
let report = analyze(&plan);

// 使用自定义配置分析
let config = DiagnosticConfig::default();
let report = analyze_with_config(&plan, &config);

// 访问诊断结果
for finding in &report.findings {
    println!("[{}] {} - {}", finding.rule_id, finding.title, finding.detail);
}
```

## 架构

Rust Cargo 工作空间，包含四个 crate：

| Crate | 类型 | 用途 |
|-------|------|------|
| [`ogexplain-core`](crates/ogexplain-core/) | 库 | 解析器 + 数据模型 + 分析引擎 + 建议引擎（无 UI 依赖） |
| [`ogexplain-cli`](crates/ogexplain-cli/) | 二进制（`ogexplain`） | CLI 前端 — 文件/管道输入，text/JSON/CSV 输出 |
| [`ogexplain-tui`](crates/ogexplain-tui/) | 二进制（`ogexplain-tui`） | 交互式 TUI — 可折叠计划树、节点详情、粘贴输入 |
| [`ogsql-complexity`](crates/ogsql-complexity/) | 库 | SQL 复杂度评分（独立可复用） |

### 核心层次

```
ogexplain-core
├── parser/          两阶段：行分类器（正则） → 树构建器（缩进栈）
├── model/           ExplainPlan → PlanNode 树，NodeType（80+ 变体），成本/统计/缓冲区类型
├── analyzer/        规则引擎，DiagnosticRule trait + DFS 遍历 + 可配置阈值 + 共享工具层
├── suggester/       诊断发现 → 优化建议映射，支持跨规则综合分析
├── summary/         SummaryRow 批量报告（SQL 复杂度 + 计划指标 + 诊断统计）
├── sql/             SQL/EXPLAIN 块分割（从混合输入中提取）
└── i18n/            基于 rust-i18n 的国际化（en, zh-CN）
```

## 诊断规则

已实现 25 条规则，分布在 17 个规则文件中，含共享工具层（`rules/utils.rs`）：

| ID | 规则 | 类别 | 说明 |
|----|------|------|------|
| SCAN-001 | 大表全表扫描 | scan | 检测 Seq Scan/PartitionedSeqScan/CStore Scan 超过行数阈值；建议 `CREATE INDEX ON table(col)` |
| SCAN-004 | 无索引过滤 | scan | 过滤大量行但缺少索引支持；提取过滤列名用于建议 |
| JOIN-001 | 大表嵌套循环 | join | 两侧行数均较高的 Nested Loop；检测内侧索引，提取连接列 |
| JOIN-002 | Hash 连接磁盘溢出 | join | Hash join 超出 work_mem；根据磁盘+内存大小计算推荐 work_mem |
| MEM-001 | 排序磁盘溢出 | memory | 外部归并排序（含 VectorSort）；报告 Sort Key |
| MEM-004 | 高峰值内存 | memory | 定位子树中最高内存节点，报告节点类型和关联表 |
| SORT-003 | 重复排序 | sort | 递归子树 Sort Key 收集；区分重复键与不同键 |
| NET-001 | 广播大量数据 | network | 跨数据节点广播过多行；支持 SplitBroadcast/PartRedistributePartBroadcast |
| EST-001 | 严重行数估算偏差 | estimation | 实际行数远超/低于优化器估算；报告偏差方向（低估/高估） |
| EST-004 | 低估导致嵌套循环 | estimation | 因行数低估导致的 Nested Loop；报告内侧处理量 |
| PUSH-001 | 查询未下推 | pushdown | FQS 失败，信号累积 — 识别具体阻断因素（SubqueryScan、SubPlan、易变函数） |
| PUSH-002 | 多层 Streaming | pushdown | 收集 Streaming 层链（`→` 标记）；按层数给出建议 |
| TYPE-001 | 隐式类型转换 | type_coercion | 基于 `TypeMismatch` 结构的检测，提供具体修复建议 |
| TYPE-004 | LIKE 前置通配符 | type_coercion | 区分单/双通配符；建议 `pg_trgm` + GIN 索引 |
| VEC-001 | 行/向量引擎混用 | vectorization | 跟踪 Row↔Vector 适配器边界，记录父→子类型 |
| GEN-001 | 执行计划过深 | general | 报告深度及原因（子查询/嵌套） |
| SUBQ-001 | 子查询未上拉 | subquery | 检测 SubqueryScan 节点；提取子表名生成参数化建议 |
| REW-001 | 大 IN 列表未改写 | subquery | 检测大量值的 IN 列表；提取列名建议改写为 `EXISTS` |
| SUBQ-006 | 关联子查询自更新 | subquery | 检测 UPDATE/DELETE 中的自引用关联子查询 |
| AGG-001 | 聚合应使用 Hash | aggregate | 对无排序需求的大 GROUP BY 建议 Hash Aggregate |
| AGG-002 | Hash 聚合磁盘溢出 | aggregate | Hash Aggregate 超出 work_mem 溢出到磁盘 |
| SKEW-001 | 数据倾斜 | distribution | 数据节点间行分布不均 |
| DIST-001 | 分布列不匹配 | distribution | 连接列与分布列不匹配导致重分布 |
| STATS-001 | 统计信息未收集 | stats | 表缺少或统计信息过期 |
| PART-001 | 分区裁剪失败 | partition | 全分区扫描，应通过裁剪减少分区 |

## OpenGauss 专属支持

本工具面向 **OpenGauss/GaussDB**（PostgreSQL 分支），而非原生 PostgreSQL。支持 OG 专属 EXPLAIN 特性：

- **向量化节点**：`Vector Hash Join`、`Vec Sort`、`Vector Sonic Hash Join/Aggregate` 等。
- **CStore 节点**：`CStore Scan`、`CStore Index Scan`（列存储扫描）。
- **Streaming 节点**：`Streaming(type: GATHER|REDISTRIBUTE|BROADCAST|...)`，含 DOP 和 NodeGroup 信息。
- **下推检测**：通过 Streaming 节点有无判断 FQS（快速查询下发）；`Data Node Scan` + `Remote query` 表示下推成功。
- **隐式类型转换**：在 `showimplicit=false` 时通过间接模式检测。
- **行/向量适配器**：`Row Adapter` / `Vector Adapter` 引擎边界标记。
- **Pretty 模式**：带 `--` 前缀的节点 ID，详细的逐节点运行时统计。
- **OG 专属属性**：Bloom Filter、Min/Max 跳过、DFS 裁剪、LLVM 优化、Skew 优化、动态 SMP、AI 预测（`p-time`、`p-rows`）。

## SQL 复杂度评分

集成的 `ogsql-complexity` crate 提供：

- **标准评分**（0–100）：基于表数量、连接数、子查询数、集合操作、CTE、窗口函数。
- **GaussDB 评分**：四维模型 — SQL 结构、PL 逻辑、高级特性、扩展功能。
- **SQL 分类**：类别（Query、DML、DDL、PL、Pkg）和子类型。
- **标签系统**：识别特定复杂度标签（如 `multi-table-join`、`correlated-subquery`、`window-function`）。

## 测试

```bash
cargo test --workspace                   # 所有测试
cargo test -p ogexplain-core            # 核心库测试
cargo test --test db_explain --features ogexplain-cli/db  # 数据库集成测试（需要 Docker）
cargo insta review                       # 交互式快照审查
cargo fmt --all && cargo clippy --workspace  # 代码检查（零警告）
```

测试 fixture 位于 `tests/fixtures/`（31 个文件） — 每个文件是原始 EXPLAIN TEXT 输出，覆盖特定场景（简单扫描、连接、溢出、流式计算、向量化、子查询、聚合、分布等）。

## 许可证

MIT
