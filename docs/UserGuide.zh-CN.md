# ogexplain-analyzer 用户手册

> OpenGauss / GaussDB 执行计划分析与性能诊断工具

---

## 目录

- [1. 简介](#1-简介)
- [2. 安装](#2-安装)
- [3. CLI 命令行使用](#3-cli-命令行使用)
  - [3.1 analyze 子命令](#31-analyze-子命令)
  - [3.2 explain 子命令](#32-explain-子命令)
  - [3.3 mcp 子命令](#33-mcp-子命令)
  - [3.4 optimize 子命令（闭环优化）](#34-optimize-子命令闭环优化)
  - [3.5 管道与标准输入](#35-管道与标准输入)
  - [3.6 国际化（i18n）](#36-国际化i18n)
- [4. TUI 交互式界面](#4-tui-交互式界面)
  - [4.1 启动模式](#41-启动模式)
  - [4.2 界面布局](#42-界面布局)
  - [4.3 按键绑定](#43-按键绑定)
  - [4.4 树面板显示](#44-树面板显示)
- [5. 结果解读](#5-结果解读)
  - [5.1 严重度级别](#51-严重度级别)
  - [5.2 诊断发现结构](#52-诊断发现结构)
  - [5.3 建议类别](#53-建议类别)
  - [5.4 跨规则综合分析](#54-跨规则综合分析)
  - [5.5 热力图](#55-热力图)
  - [5.6 瀑布图](#56-瀑布图)
- [6. MCP 与 AI 助手集成](#6-mcp-与-ai-助手集成)
  - [6.1 什么是 MCP](#61-什么是-mcp)
  - [6.2 配置方法](#62-配置方法)
  - [6.3 五个工具详解](#63-五个工具详解)
  - [6.4 gaussdb-mcp 集成工作流](#64-gaussdb-mcp-集成工作流)
- [7. 常见工作流](#7-常见工作流)
- [8. 常见问题排查](#8-常见问题排查)

---

## 1. 简介

### 1.1 本工具是什么

ogexplain-analyzer 是一个专为 **OpenGauss / GaussDB** 数据库设计的 `EXPLAIN` / `EXPLAIN ANALYZE` 输出解析与性能诊断工具。它能够：

- **解析执行计划**：完整解析 TEXT 格式的 EXPLAIN 输出，包括 OpenGauss 专属特性（向量化节点、CStore 列存储、Streaming 流式算子、Pretty 模式等）。
- **自动诊断**：运行 25 条诊断规则，覆盖全表扫描、连接策略、内存溢出、排序重复、网络广播、估算偏差、下推失败、隐式类型转换、行/向量引擎混用、子查询优化、聚合策略、数据倾斜、分布列不匹配、统计信息缺失、分区裁剪失败等场景。
- **参数化建议**：从计划属性中提取表名、列名、具体数值，生成可操作的优化建议（如 `CREATE INDEX ON orders(status)`）。
- **可视化分析**：提供成本偏差热力图和资源瀑布图，直观展示估算准确性和 CPU/内存瓶颈。
- **SQL 复杂度评分**：集成 ogsql-complexity，按 0–100 分制评估 SQL 语句复杂度，支持 GaussDB 四维模型。
- **多种接口**：CLI（脚本集成）、TUI（交互式探索）、MCP 服务器（AI 助手集成）、库 crate（嵌入式调用）。

### 1.2 面向谁

本工具面向以下用户：

- **数据库管理员（DBA）**：快速定位执行计划中的性能瓶颈，获取优化建议。
- **应用开发人员**：理解查询执行路径，验证索引使用情况，优化 SQL 语句。
- **性能工程师**：批量分析多个执行计划，导出 CSV 进行对比分析。
- **AI 辅助开发用户**：通过 MCP 服务器将分析能力集成到 Claude、Cursor 等 AI 工具中。

### 1.3 与 PostgreSQL EXPLAIN 工具的区别

本工具专门针对 OpenGauss/GaussDB（PostgreSQL 分支）设计，而非原生 PostgreSQL。它额外支持：

| 特性 | 说明 |
|------|------|
| 向量化节点 | `Vector Hash Join`、`Vec Sort`、`Vector Sonic Hash Join/Aggregate` 等 |
| CStore 节点 | `CStore Scan`、`CStore Index Scan`（列存储扫描） |
| Streaming 节点 | `Streaming(type: GATHER\|REDISTRIBUTE\|BROADCAST\|...)`，含 DOP 和 NodeGroup 信息 |
| 下推检测 | 通过 Streaming 节点有无判断 FQS（快速查询下发）是否成功 |
| 隐式类型转换 | 在 `showimplicit=false` 时通过间接模式检测 |
| 行/向量适配器 | `Row Adapter` / `Vector Adapter` 引擎边界标记 |
| Pretty 模式 | 带 `--` 前缀的节点 ID，详细的逐节点运行时统计 |

---

## 2. 安装

### 2.1 环境要求

- **操作系统**：Linux、macOS、Windows（通过 WSL）
- **Rust**：1.70 或更高版本（推荐使用 `rustup` 安装最新稳定版）
- **Git**：用于克隆仓库

安装 Rust 工具链：

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source $HOME/.cargo/env
```

### 2.2 从源码构建

```bash
# 克隆仓库
git clone https://github.com/c2j/ogexplain-analyzer.git
cd ogexplain-analyzer

# 构建 release 版本（推荐）
cargo build --release

# 或者构建 debug 版本（构建更快，但运行较慢）
cargo build
```

### 2.3 二进制文件位置

构建完成后，二进制文件位于：

| 二进制 | 路径 | 用途 |
|--------|------|------|
| `ogexplain` | `target/release/ogexplain` | CLI 命令行工具 |
| `ogexplain-tui` | `target/release/ogexplain-tui` | 交互式 TUI 界面 |
| `ogexplain-mcp` | `target/release/ogexplain-mcp` | MCP 服务器（供 AI 助手使用） |

如需全局使用，可以将二进制文件复制到 PATH 路径下：

```bash
cp target/release/ogexplain /usr/local/bin/
cp target/release/ogexplain-tui /usr/local/bin/
cp target/release/ogexplain-mcp /usr/local/bin/
```

或者使用 `cargo install` 从本地路径安装：

```bash
cargo install --path crates/ogexplain-cli
cargo install --path crates/ogexplain-tui
cargo install --path crates/ogexplain-mcp
```

### 2.4 验证安装

```bash
ogexplain --version
# 输出示例：ogexplain 0.x.x

ogexplain-tui --version
# 输出示例：ogexplain-tui 0.x.x
```

---

## 3. CLI 命令行使用

CLI 工具名为 `ogexplain`，提供四个子命令：`analyze`、`explain`、`optimize` 和 `mcp`。

```bash
ogexplain <子命令> [选项]
```

### 3.1 analyze 子命令

分析 EXPLAIN 输出文件或标准输入。

```bash
ogexplain analyze <文件路径> [选项]
```

#### 参数说明

| 参数 | 默认值 | 说明 |
|------|--------|------|
| `-o, --output` | `text` | 输出格式：`text`、`json`、`heatmap`、`waterfall` |
| `--threshold` | `info` | 最低严重级别：`critical`、`warning`、`info` |
| `-q, --quiet` | — | 仅显示诊断结果，不显示执行计划树 |
| `-v, --verbose` | — | 详细输出 |
| `--multi` | — | 启用多块解析（混合 SQL + EXPLAIN 文件） |
| `--csv <路径>` | — | 导出汇总表为 CSV（使用 `-` 输出到 stdout） |
| `--lang` | `auto` | 输出语言：`en`、`zh-CN` 或 `auto`（跟随系统语言） |

#### 输出格式

**text（默认）**

人类可读的文本报告，包含执行计划树、诊断发现（按严重度分组）、优化建议和 SQL 复杂度分析。

```bash
ogexplain analyze tests/fixtures/03_hash_join.txt
```

输出示例：

```
══════════════════════════════════════════════
  OpenGauss Execution Plan Analysis Report
══════════════════════════════════════════════

执行计划树
─────────
Hash Join  (cost=...  actual=...)
  Hash Cond: ...
  -> Seq Scan on orders  (...)
  -> Hash  (...)
     -> Seq Scan on customers  (...)

🔴 Critical (1)
──────────────────
  [SCAN-001] 大表全表扫描
    Node: "Seq Scan" (line 3)
    orders 表执行了全表扫描，扫描行数 1,500,000 超过阈值
    建议：CREATE INDEX ON orders(customer_id)

💡 建议
──────────────
  1. [High] 考虑在 orders(customer_id) 上创建索引以避免全表扫描
```

**json**

结构化 JSON 输出，包含计划树、诊断发现、建议、统计、复杂度、热力图和瀑布图数据。适合脚本处理和工具集成。

```bash
ogexplain analyze tests/fixtures/03_hash_join.txt -o json
```

**heatmap**

成本-实际偏差热力图，按 Q-Error 严重级别展示每个节点的估算准确性。**需要 EXPLAIN ANALYZE 输出**（含实际执行统计）。

```bash
ogexplain analyze tests/fixtures/10_complex_plan.txt -o heatmap
```

**waterfall**

资源瀑布图，以百分比条展示 CPU/内存瓶颈，标识最慢/最热的节点。**需要 EXPLAIN ANALYZE 输出**。

```bash
ogexplain analyze tests/fixtures/10_complex_plan.txt -o waterfall
```

#### 严重级别过滤

使用 `--threshold` 过滤显示的最低严重级别：

```bash
# 仅显示严重（Critical）级别发现
ogexplain analyze file.txt --threshold critical

# 显示严重和警告级别（Critical + Warning）
ogexplain analyze file.txt --threshold warning

# 显示所有级别（默认）
ogexplain analyze file.txt --threshold info
```

#### 批量模式与 CSV 导出

当输入文件包含多个 SQL 和 EXPLAIN 块时，使用 `--multi` 启用多块解析：

```bash
ogexplain analyze multi_block.txt --multi --csv results.csv
```

CSV 导出包含 43 列，涵盖 SQL 复杂度、计划指标、诊断统计等。使用 `-` 将 CSV 输出到标准输出：

```bash
ogexplain analyze file.txt --csv - | head -1
```

### 3.2 explain 子命令

直接连接 OpenGauss/GaussDB 数据库，执行 `EXPLAIN [ANALYZE]` 并一步完成分析。**需要编译时启用 `db` feature**（默认已启用）。

```bash
ogexplain explain -s "<SQL 语句>" [选项]
```

连接信息从配置文件（`--config <path>`，默认 `~/.gaussdb.toml`）或 `GAUSSDB_URL` / `DATABASE_URL` 环境变量加载。已移除 `-d/--dsn` 选项，避免凭据出现在命令行 / shell 历史 / `ps` 输出中。

#### 参数说明

| 参数 | 说明 |
|------|------|
| `--config <path>` | TOML 配置文件路径（默认：`~/.gaussdb.toml`） |
| `--name <name>` | 多连接配置中的命名连接 |
| `-s, --sql` | 内联 SQL 语句 |
| `-f, --sql-file` | SQL 文件路径 |
| `--analyze` | 执行 EXPLAIN ANALYZE（会实际执行查询） |
| `-o, --output` | 输出格式：`text`、`json`、`heatmap`、`waterfall` |
| `--threshold` | 最低严重级别：`critical`、`warning`、`info` |
| `-q, --quiet` | 仅显示诊断结果 |
| `--csv <路径>` | 导出汇总表为 CSV |
| `--lang` | 输出语言 |

#### 使用示例

**基本 EXPLAIN（仅查看计划，不执行查询）：**

```bash
# 使用默认配置路径 ~/.gaussdb.toml
ogexplain explain -s "SELECT * FROM orders WHERE status = 'pending'"

# 显式指定配置文件
ogexplain explain --config /etc/ogexplain/prod.toml -s "SELECT * FROM orders WHERE status = 'pending'"
```

**EXPLAIN ANALYZE（实际执行查询并分析）：**

```bash
ogexplain explain \
    -s "SELECT COUNT(*) FROM orders WHERE create_date > '2025-01-01'" \
    --analyze
```

> **注意：** `--analyze` 会在数据库上实际执行查询。在生产系统上请谨慎使用，避免对大表执行修改操作。

**从文件读取 SQL：**

```bash
ogexplain explain -f query.sql
```

**指定命名连接（多连接配置）：**

```bash
ogexplain explain --name prod -s "SELECT ..."
```

**带完整分析选项：**

```bash
ogexplain explain \
    --name prod \
    -s "SELECT * FROM orders JOIN customers ON orders.customer_id = customers.id WHERE orders.status = 'pending'" \
    -o json --csv results.csv --threshold warning
```

**配置文件格式**

`~/.gaussdb.toml`（与 `gaussdb-mcp` 工具共享）支持扁平单连接或 `[[connections]]` 多连接形式：

```toml
# 扁平单连接
host = "192.168.1.100"
port = 5432
dbname = "mydb"
user = "gaussdb"
password = "secret"   # 或 "keyring" 表示从 OS keychain 读取
sslmode = "disable"
```

sslmode 可选值：`disable`、`allow`、`prefer`、`require`、`verify-ca`、`verify-full`。

### 3.3 mcp 子命令

启动 MCP（Model Context Protocol）服务器，供 AI 助手通过 stdio 传输调用。**需要编译时启用 `mcp` feature**。

```bash
# 构建带 MCP 支持的 CLI
cargo build -p ogexplain-cli --features mcp

# 启动 MCP 服务器
ogexplain mcp
```

MCP 服务器的详细使用方法请参见[第 6 章](#6-mcp-与-ai-助手集成)。

### 3.4 optimize 子命令（闭环优化）

`optimize` 子命令运行基于 [metamorphosis](https://github.com/c2j/metamorphosis) 的迭代闭环 SQL 优化管道。使用库 API（而非子进程）进行 SQL 重写和语义等价验证。

**管道**：`EXPLAIN → 诊断 → 映射到重写规则 → metamorphosis 重写 → QED/VeriEQL 验证 → 重新 EXPLAIN → 收敛`

```bash
# 构建时已默认启用数据库支持
cargo build -p ogexplain-cli

# 优化 SQL 语句（内联）
ogexplain optimize -s "SELECT * FROM orders o WHERE EXISTS (SELECT 1 FROM users u WHERE u.id = o.uid)"

# 从文件读取
ogexplain optimize -f query.sql

# 指定命名连接和 Schema 文件（上下文感知重写）
ogexplain optimize -s "SELECT ..." --name ogagila --schema schema.json

# 限制迭代次数并跳过验证（快速迭代）
ogexplain optimize -s "SELECT ..." --max-iterations 3 --skip-verify

# JSON 输出
ogexplain optimize -s "SELECT ..." --format json -o result.json
```

#### 参数说明

| 参数 | 默认值 | 说明 |
|------|--------|------|
| `-s, --sql <sql>` | — | 要优化的 SQL 语句（内联字符串） |
| `-f, --sql-file <path>` | — | 包含 SQL 语句的文件 |
| `--config <path>` | `~/.gaussdb.toml` | DB 配置文件路径 |
| `--name <name>` | — | 配置文件中的命名连接 |
| `--schema <path>` | — | Schema JSON 文件（`{table: {col: type}}`）用于重写上下文 |
| `--sql-dir <path>` | — | `.sql` DDL 文件目录（替代 `--schema`） |
| `--max-iterations <n>` | `10` | 强制停止前的最大迭代次数 |
| `--skip-verify` | — | 跳过语义等价验证 |
| `--verify-engine <name>` | `qed` | 验证引擎：`qed`（形式化 Z3 证明）或 `verieql`（有界检查） |
| `--verify-timeout <s>` | `60` | 每次重写的验证超时（秒） |
| `--verify-bound <n>` | `2` | VeriEQL 边界参数（每个表的最大行数） |
| `--format <fmt>` | `text` | 输出格式：`text`、`json` |
| `-o, --output <path>` | — | 输出文件路径 |
| `-v, --verbose` | — | 详细输出 |

#### 收敛条件

当满足以下任一条件时，优化循环停止：

| 停止原因 | 说明 |
|----------|------|
| **Success** | 所有关键发现已解决（critical_count = 0） |
| **FixedPoint** | 重写后的 SQL 与之前已见过的 SQL 相同 |
| **NoRewritableFindings** | 没有诊断发现可以映射到重写规则 |
| **Regression** | 成本退化超过阈值（`regression_threshold_pct` = 10%） |
| **Plateau** | 连续 `max_plateau_count`（3）次迭代改进低于 `min_improvement_pct`（5%） |
| **MaxIterations** | 达到 `--max-iterations` |
| **VerificationFailed** | QED/VeriEQL 验证不通过 |

#### 输出示例（text 格式）

```
=== 优化报告 ===
停止原因: FixedPoint
迭代次数: 1

--- 第 1 次迭代 ---
触发规则: SUBQ-001 (重写规则: ["subquery-to-join"])
成本: 500.00 → 200.00 (-60.0%)
关键发现: 2 → 0
验证: (未执行)

=== 最终 SQL ===
SELECT o.id, o.amount FROM orders AS o INNER JOIN users AS u ON u.id = o.user_id AND u.active = 1
```

#### Schema 格式

`--schema` 选项为 metamorphosis 提供表结构信息，用于上下文感知重写（如 `SELECT *` 展开）：

```json
{
  "users": { "id": "INTEGER", "name": "VARCHAR(100)", "email": "VARCHAR(255)" },
  "orders": { "id": "INTEGER", "user_id": "INTEGER", "amount": "NUMERIC(10,2)" }
}
```

支持 `primary_key`（增强 QED 验证准确性）：

```json
{
  "users": {
    "columns": { "id": "INTEGER", "name": "VARCHAR(100)" },
    "primary_key": ["id"]
  }
}
```

### 3.5 管道与标准输入

`analyze` 子命令支持从标准输入读取数据（文件路径参数使用 `-`）：

```bash
# 从管道读取
cat explain_output.txt | ogexplain analyze -

# 从另一个命令的输出读取
gsql -d mydb -c "EXPLAIN SELECT * FROM orders" | ogexplain analyze -

# 带选项的管道使用
gsql -d mydb -c "EXPLAIN ANALYZE SELECT * FROM orders" | ogexplain analyze - -o json
```

### 3.6 国际化（i18n）

ogexplain-analyzer 支持中文（`zh-CN`）和英文（`en`）两种输出语言。

**通过命令行参数指定：**

```bash
# 中文输出
ogexplain analyze file.txt --lang zh-CN

# 英文输出
ogexplain analyze file.txt --lang en
```

**自动检测（默认）：**

使用 `--lang auto`（默认值）时，工具会自动检测系统语言设置。如果系统语言为中文，则输出中文；否则输出英文。

```bash
# 自动检测（默认行为）
ogexplain analyze file.txt
```

---

## 4. TUI 交互式界面

TUI（Terminal User Interface）提供了一个基于终端的交互式执行计划浏览器，基于 ratatui 构建。

### 4.1 启动模式

TUI 有两种启动模式：

**文件加载模式** — 加载 EXPLAIN 输出文件并自动解析：

```bash
ogexplain-tui tests/fixtures/10_complex_plan.txt
```

**粘贴模式** — 启动空白界面，粘贴 EXPLAIN 文本后手动触发解析：

```bash
ogexplain-tui
```

在粘贴模式下，将 EXPLAIN 输出粘贴到输入区域，然后按 `Ctrl+P` 触发解析。

**命令模式**

在输入区域中可以键入命令（以 `:` 开头）：

| 命令 | 说明 |
|------|------|
| `:load <路径>` | 从磁盘加载文件 |
| `:quit` 或 `:q` | 退出 TUI |

示例：

```
:load /path/to/explain_output.txt
:quit
```

### 4.2 界面布局

TUI 界面由以下面板组成：

```
┌─────────────────────────────────────────────────────────┐
│  标题栏                                                  │
├─────────────────────────────────────────────────────────┤
│  摘要面板（Summary）                                      │
│  显示计划总行数、节点数、总耗时、峰值内存等关键指标           │
├──────────────────────────┬──────────────────────────────┤
│  树面板（Tree）           │  详情面板（Detail）            │
│                          │                              │
│  可折叠的计划节点树       │  当前选中节点的详细信息        │
│  带严重度图标和类别颜色   │  含属性、成本、缓冲区统计      │
│                          │                              │
├──────────────────────────┴──────────────────────────────┤
│  输入面板（Input）                                        │
│  粘贴 EXPLAIN 文本或输入命令                              │
├─────────────────────────────────────────────────────────┤
│  状态栏（Status）                                         │
│  显示当前状态、按键提示                                   │
└─────────────────────────────────────────────────────────┘
```

各面板说明：

| 面板 | 说明 |
|------|------|
| **标题栏** | 显示工具名称和版本号 |
| **摘要面板** | 显示执行计划关键指标：节点数、总运行时间、峰值内存、缓冲区命中率等 |
| **树面板** | 可折叠的计划节点树，每个节点带严重度图标和类别颜色 |
| **详情面板** | 显示选中节点的完整信息，包括属性、估算成本、实际统计、缓冲区信息、诊断发现 |
| **输入面板** | 多行文本区域，用于粘贴 EXPLAIN 输出或输入命令 |
| **状态栏** | 显示当前焦点面板、按键提示、诊断计数 |

### 4.3 按键绑定

TUI 的按键绑定按功能类别组织如下：

#### 全局按键

| 按键 | 操作 | 说明 |
|------|------|------|
| `Ctrl+P` | 解析 EXPLAIN 文本 | 解析输入区域中的 EXPLAIN 内容 |
| `Ctrl+L` | 清空输入并重置 | 清除输入区域，重置当前分析状态 |
| `Ctrl+C` | 退出 | 立即退出 TUI |
| `?` 或 `F1` | 切换帮助覆盖层 | 显示/隐藏帮助信息 |
| `q` | 退出 | 在非输入模式下退出（避免误触） |

#### 面板导航

| 按键 | 操作 | 说明 |
|------|------|------|
| `Tab` | 循环焦点 | 按顺序切换：树面板 → 详情面板 → 输入面板 → 树面板 |
| `Shift+Tab` | 反向循环焦点 | 按反序切换面板焦点 |

#### 树面板操作（树面板获得焦点时）

| 按键 | 操作 | 说明 |
|------|------|------|
| `↑` / `k` | 上移 | 选中上一个节点 |
| `↓` / `j` | 下移 | 选中下一个节点 |
| `g` | 跳到顶部 | 选中第一个节点 |
| `G` | 跳到底部 | 选中最后一个节点 |
| `Enter` | 展开/折叠 | 切换当前节点的展开/折叠状态 |
| `E` | 展开所有节点 | 展开整棵树的所有节点 |
| `W` | 折叠所有节点 | 折叠整棵树的所有节点 |

#### 详情面板操作（详情面板获得焦点时）

| 按键 | 操作 | 说明 |
|------|------|------|
| `↑` / `k` | 上滚 | 向上滚动一行 |
| `↓` / `j` | 下滚 | 向下滚动一行 |
| `PgUp` | 上翻页 | 向上翻页 |
| `PgDn` | 下翻页 | 向下翻页 |
| `Home` | 跳到顶部 | 滚动到详情顶部 |
| `End` | 跳到底部 | 滚动到详情底部 |

#### 视图切换

| 按键 | 操作 | 说明 |
|------|------|------|
| `r` | 切换原始 EXPLAIN 视图 | 在详情面板中显示原始 EXPLAIN 文本 |
| `c` | 切换 SQL 复杂度分析 | 显示/隐藏 SQL 复杂度评分和维度分析 |
| `F` | 切换节点诊断/全部发现 | 在当前节点诊断和全部发现之间切换 |

#### 多计划导航

当文件包含多个 EXPLAIN 块时：

| 按键 | 操作 | 说明 |
|------|------|------|
| `N` 或 `n` | 下一个计划 | 切换到下一个 EXPLAIN 计划 |
| `P` 或 `p` | 上一个计划 | 切换到上一个 EXPLAIN 计划 |

### 4.4 树面板显示

树面板中的每个节点显示以下视觉标记：

**严重度图标：**

| 图标 | 严重度 | 颜色 | 含义 |
|------|--------|------|------|
| `!!` | Critical（严重） | 红色 | 需要立即关注的性能问题 |
| `!` | Warning（警告） | 黄色 | 建议优化的潜在问题 |
| `*` | Info（信息） | 绿色 | 值得关注的优化提示 |

**类别颜色：**

| 颜色 | 类别 | 说明 |
|------|------|------|
| 蓝色 | 扫描（Scan） | Seq Scan、Index Scan、CStore Scan 等 |
| 品红 | 连接（Join） | Hash Join、Nested Loop、Merge Join 等 |
| 青色 | 聚合（Aggregate） | HashAggregate、GroupAggregate 等 |
| 黄色 | 排序（Sort） | Sort、External Sort 等 |
| 绿色 | DML | Insert、Update、Delete 等 |
| 红色 | Streaming | GATHER、REDISTRIBUTE、BROADCAST 等 |

**展开/折叠标记：**

| 符号 | 含义 |
|------|------|
| `▾` | 节点已展开，子节点可见 |
| `▸` | 节点已折叠，子节点隐藏 |
| `·` | 叶节点，无子节点 |

**树面板示例：**

```
!! Hash Join on orders                      ← 严重，品红色
  ▾   !! Seq Scan on orders                 ← 严重，蓝色
  ·       Filter: (status = 'pending')
  ▸   *  Hash                               ← 信息
  ▸      * Seq Scan on customers            ← 信息
```

---

## 5. 结果解读

### 5.1 严重度级别

每条诊断发现都有一个严重度级别，含义如下：

| 级别 | 英文 | 含义 | 建议处理方式 |
|------|------|------|------------|
| **严重** | Critical | 可能导致严重性能问题的发现，如大表全表扫描、磁盘溢出、估算严重偏差 | **优先处理**，建议立即优化 |
| **警告** | Warning | 可能影响性能的潜在问题，如排序溢出、连接策略不佳、数据倾斜 | **建议优化**，可在下一个维护窗口处理 |
| **信息** | Info | 值得关注但不紧急的优化提示，如统计信息过期、分区裁剪建议 | **记录并跟踪**，择机优化 |

### 5.2 诊断发现结构

每条诊断发现包含以下字段：

| 字段 | 说明 |
|------|------|
| `rule_id` | 规则 ID，如 `SCAN-001`、`JOIN-001`、`EST-001` |
| `title` | 发现标题，简要描述问题 |
| `detail` | 详细描述，包含具体的表名、列名、数值等参数化信息 |
| `suggestion` | 优化建议（可选），提供具体的操作步骤 |
| `severity` | 严重度级别：Critical、Warning、Info |
| `node_type` | 触发规则的节点类型（如 `Seq Scan`、`Hash Join`） |
| `node_line` | 在 EXPLAIN 输出中的行号 |
| `sql_rewrite` | SQL 改写建议（可选），含改写后的 SQL 和说明 |

**示例：**

```
[SCAN-001] 大表全表扫描
  Node: "Seq Scan" (line 5)
  orders 表执行了全表扫描，扫描行数 2,500,000 超过阈值 100,000
  建议：CREATE INDEX ON orders(status)
```

### 5.3 建议类别

工具将诊断发现映射为以下五类优化建议：

| 类别 | 英文 | 说明 | 典型建议 |
|------|------|------|---------|
| **索引优化** | IndexOptimization | 通过添加或调整索引来改善查询性能 | `CREATE INDEX ON table(column)` |
| **统计信息更新** | StatisticsUpdate | 更新表统计信息以帮助优化器做出更好的决策 | `ANALYZE table` |
| **查询改写** | QueryRewrite | 通过改写 SQL 语句来改善执行计划 | 将 `IN (SELECT ...)` 改为 `EXISTS` |
| **配置调优** | ConfigurationTuning | 调整数据库配置参数 | 增大 `work_mem`、调整 `enable_nestloop` |
| **分布优化** | DistributionOptimization | 优化数据分布策略 | 调整分布列、使用 Replicate 表 |

### 5.4 跨规则综合分析

工具不仅提供单条规则的诊断，还会进行跨规则综合分析，将多个相关发现映射为高层建议：

| 综合模式 | 触发条件 | 建议 |
|---------|---------|------|
| **多节点磁盘溢出** | 多个 Sort/Hash 节点溢出到磁盘 | 建议全局增大 `work_mem` 参数 |
| **多节点估算偏差** | 多个节点估算/实际行数偏差大 | 建议执行 `ANALYZE` 更新统计信息 |
| **扫描+连接问题** | 全表扫描 + 连接列无索引 | 建议在连接列上创建复合索引 |
| **类型一致性问题** | 多处隐式类型转换 | 建议统一列类型，避免隐式转换 |
| **引擎混用** | 行引擎和向量引擎交替出现 | 建议统一使用一种引擎以避免适配器开销 |

### 5.5 热力图

热力图展示每个计划节点的成本-实际偏差，按 Q-Error（估算偏差比）分级：

**Q-Error 级别：**

| 级别 | Q-Error 范围 | 含义 | 处理建议 |
|------|-------------|------|---------|
| **可忽略** | < 2 | 估算与实际接近 | 无需处理 |
| **轻微** | 2 – 5 | 轻微偏差 | 关注但不紧急 |
| **中等** | 5 – 10 | 中等偏差 | 建议更新统计信息 |
| **严重** | 10 – 100 | 严重偏差 | 优先更新统计信息 |
| **极端** | > 100 | 估算严重偏离实际 | 立即处理，可能影响执行计划选择 |

**使用热力图：**

```bash
# 生成热力图（需要 EXPLAIN ANALYZE 输出）
ogexplain analyze explain_analyze_output.txt -o heatmap
```

热力图会为每个节点显示：节点类型、估算行数、实际行数、Q-Error 值、偏差方向（低估/高估）和严重级别。

### 5.6 瀑布图

瀑布图以百分比条的形式展示 CPU 和内存瓶颈：

**CPU 瓶颈：** 标识执行时间占比最高的节点，帮助快速定位耗时最长的操作。

**内存瓶颈：** 标识内存使用最高的节点，帮助发现内存热点。

```bash
# 生成瀑布图（需要 EXPLAIN ANALYZE 输出）
ogexplain analyze explain_analyze_output.txt -o waterfall
```

瀑布图会为每个节点显示：节点类型、关联表、CPU 占比条、内存占比条和绝对数值。

---

## 6. MCP 与 AI 助手集成

### 6.1 什么是 MCP

MCP（Model Context Protocol）是一种标准化的协议，允许 AI 助手（如 Claude、Cursor、VS Code 中的 Copilot）调用外部工具。ogexplain-analyzer 通过 MCP 服务器将其分析能力暴露给 AI 助手，实现以下工作流：

1. AI 助手接收用户的 SQL 性能问题
2. AI 助手调用 `analyze_explain` 工具分析执行计划
3. AI 助手基于诊断结果和优化建议，向用户提供具体的优化方案

### 6.2 配置方法

#### Claude Desktop 配置

编辑 Claude Desktop 配置文件（路径因平台而异）：

- **macOS**：`~/Library/Application Support/Claude/claude_desktop_config.json`
- **Windows**：`%APPDATA%\Claude\claude_desktop_config.json`

添加以下内容：

```json
{
  "mcpServers": {
    "ogexplain": {
      "command": "/path/to/ogexplain-mcp",
      "args": []
    }
  }
}
```

如果 `ogexplain-mcp` 已在 PATH 中，可以直接使用：

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

#### Cursor 配置

在 Cursor 的设置中，找到 MCP 服务器配置，添加：

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

#### VS Code（Copilot）配置

在 VS Code 的 `settings.json` 中添加 MCP 服务器配置（具体路径取决于所使用的 MCP 扩展）：

```json
{
  "mcp": {
    "servers": {
      "ogexplain": {
        "command": "ogexplain-mcp",
        "args": []
      }
    }
  }
}
```

### 6.3 五个工具详解

MCP 服务器提供以下 5 个工具：

#### analyze_explain

**用途**：解析 EXPLAIN 文本并运行完整的诊断分析。

**输入**：
- `explain_text`（必需）：EXPLAIN 或 EXPLAIN ANALYZE 的 TEXT 输出
- `sql_text`（可选）：原始 SQL 语句（用于 SQL 改写支持）

**输出**：
- 诊断发现列表（含 rule_id、title、detail、suggestion、severity）
- 优化建议列表
- 文本摘要

**使用场景**：当你有一个 EXPLAIN 输出文本，需要获取完整的诊断分析结果时。

#### parse_explain

**用途**：仅解析 EXPLAIN 文本，返回结构化的计划树。

**输入**：
- `explain_text`（必需）：EXPLAIN TEXT 输出

**输出**：
- 结构化的 JSON 计划树，包含节点类型、成本、统计、缓冲区信息

**使用场景**：当你只需要解析计划结构，不需要运行诊断时。

#### list_diagnostic_rules

**用途**：列出所有 25 条诊断规则。

**输入**：无

**输出**：
- 规则列表，含 ID、类别、描述

**使用场景**：了解工具支持哪些诊断规则，或筛选特定类别的规则。

#### get_suggestions

**用途**：获取跨规则综合建议。

**输入**：
- `findings`（必需）：诊断发现列表

**输出**：
- 综合建议列表，含建议类别、置信度评分、具体操作

**使用场景**：当已有诊断发现列表，需要获取高层优化建议时。

**置信度评分：**

| 评分 | 含义 |
|------|------|
| >= 0.85 | 高置信度，强烈建议执行 |
| 0.7 – 0.85 | 中等置信度，建议考虑 |
| < 0.7 | 低置信度，仅供参考 |

#### score_sql_complexity

**用途**：评估 SQL 语句复杂度。

**输入**：
- `sql`（必需）：SQL 语句文本

**输出**：
- 标准评分（0–100）和复杂度级别
- GaussDB 四维评分：SQL 结构、PL 逻辑、高级特性、扩展功能
- SQL 分类（Query、DML、DDL、PL、Pkg）和子类型
- 复杂度标签（如 `multi-table-join`、`correlated-subquery`、`window-function`）

**使用场景**：评估 SQL 复杂度，作为代码审查或性能基线的一部分。

### 6.4 gaussdb-mcp 集成工作流

通过结合 `gaussdb-mcp`（GaussDB 数据库 MCP 服务器）和 `ogexplain-mcp`，可以实现端到端的 SQL 性能诊断：

**工作流程：**

```
用户提问 → AI 助手
              ↓
         gaussdb-mcp
         （连接数据库，执行 EXPLAIN）
              ↓
         获取 EXPLAIN 输出
              ↓
         ogexplain-mcp
         （解析 + 诊断 + 建议）
              ↓
         返回分析结果
              ↓
         AI 助手整合结果，给出优化方案
```

**配置示例（同时使用两个 MCP 服务器）：**

```json
{
  "mcpServers": {
    "gaussdb": {
      "command": "gaussdb-mcp",
      "args": ["--host", "192.168.1.100", "--port", "5432"]
    },
    "ogexplain": {
      "command": "ogexplain-mcp",
      "args": []
    }
  }
}
```

**AI 助手操作示例：**

1. 用户："帮我分析这个查询的性能问题：`SELECT * FROM orders WHERE status = 'pending'`"
2. AI 助手调用 `gaussdb-mcp` 的 `execute_query` 工具执行 `EXPLAIN ANALYZE SELECT * FROM orders WHERE status = 'pending'`
3. AI 助手将 EXPLAIN 输出传递给 `ogexplain-mcp` 的 `analyze_explain` 工具
4. AI 助手基于诊断结果向用户展示优化建议

---

## 7. 常见工作流

### 7.1 快速检查

最简单的用法——分析一个 EXPLAIN 输出文件：

```bash
ogexplain analyze explain_output.txt
```

输出包含计划树、诊断发现和建议。

### 7.2 详细 JSON 导出配合 jq

导出 JSON 格式并用 `jq` 筛选特定信息：

```bash
# 导出完整 JSON
ogexplain analyze explain_output.txt -o json > analysis.json

# 仅提取严重级别的发现
cat analysis.json | jq '.findings[] | select(.severity == "Critical")'

# 提取所有建议
cat analysis.json | jq '.suggestions[]'

# 查看特定规则的发现
cat analysis.json | jq '.findings[] | select(.rule_id == "SCAN-001")'

# 统计各类别的发现数量
cat analysis.json | jq '.findings | group_by(.category) | map({category: .[0].category, count: length})'
```

### 7.3 批量 CSV 报表

分析包含多个 SQL 和 EXPLAIN 块的文件，导出为 CSV 用于电子表格分析：

```bash
# 批量分析并导出 CSV
ogexplain analyze multi_statements.txt --multi --csv batch_report.csv

# 也可以输出到 stdout 并管道到其他工具
ogexplain analyze multi_statements.txt --multi --csv - | column -t -s,
```

CSV 文件包含 43 列，涵盖：SQL 预览、复杂度评分、计划指标（成本、时间、行数）、诊断统计（严重/警告/信息计数）等。

### 7.4 数据库直连 EXPLAIN ANALYZE

直接连接数据库，执行 EXPLAIN ANALYZE 并一步完成分析：

```bash
# 基本用法（连接信息从 ~/.gaussdb.toml 读取）
ogexplain explain \
    -s "SELECT * FROM orders WHERE status = 'pending'" \
    --analyze

# 导出为 JSON
ogexplain explain \
    -s "SELECT * FROM orders WHERE status = 'pending'" \
    --analyze -o json > full_analysis.json

# 从 gsql 管道
gsql -d mydb -c "EXPLAIN ANALYZE SELECT * FROM orders WHERE status = 'pending'" | \
    ogexplain analyze -
```

> **注意：** `EXPLAIN ANALYZE` 会实际执行查询。对于 `SELECT` 语句通常安全，但避免对 `UPDATE`/`DELETE` 使用 `--analyze`，除非在测试环境中。

### 7.5 闭环 SQL 优化

基于诊断发现迭代重写 SQL，直到收敛：

```bash
# 优化子查询 — metamorphosis 将 EXISTS 重写为 JOIN
ogexplain optimize \
    -s "SELECT * FROM orders o WHERE EXISTS (SELECT 1 FROM users u WHERE u.id = o.uid)" \
    --name ogagila

# 带 Schema 的上下文感知重写 + QED 验证
ogexplain optimize \
    -s "SELECT * FROM orders WHERE film_id IN (SELECT film_id FROM film_actor)" \
    --schema schema.json --verify-engine qed --name ogagila

# 快速迭代模式 — 跳过验证、限制迭代次数
ogexplain optimize \
    -f query.sql --max-iterations 3 --skip-verify --name ogagila
```

**典型工作流程：**

1. 编写 SQL 查询语句
2. 运行 `ogexplain optimize -s "你的 SQL" --name <连接>`
3. 查看优化报告 — 触发了哪些规则、重写结果如何
4. 对比重写前后的成本变化
5. 可选择启用验证（`--verify-engine qed`）获取形式化等价证明

### 7.7 TUI 交互式分析

使用 TUI 进行交互式执行计划探索：

```bash
# 加载文件
ogexplain-tui explain_output.txt

# 粘贴模式
ogexplain-tui
```

**典型 TUI 操作流程：**

1. 启动 TUI 并加载 EXPLAIN 输出
2. 在树面板中使用 `↑↓` 浏览节点，`Enter` 展开/折叠
3. 切换到详情面板（`Tab`），查看选中节点的完整属性和诊断信息
4. 按 `c` 查看 SQL 复杂度分析
5. 按 `r` 查看原始 EXPLAIN 文本
6. 如果有多个计划，按 `N`/`P` 切换
7. 按 `q` 退出

### 7.8 AI 辅助分析

通过 MCP 服务器，在 AI 助手中获取智能分析：

```bash
# 确保 MCP 服务器可用
ogexplain-mcp

# 或通过 CLI 启动
ogexplain mcp
```

在 Claude Desktop 或 Cursor 中直接对话：

- "分析这个 EXPLAIN 输出并告诉我有什么性能问题"
- "这个查询为什么使用了 Nested Loop 而不是 Hash Join？"
- "帮我优化这个 SQL，它的执行时间太长了"

---

## 8. 常见问题排查

### 8.1 解析错误

**问题**：运行 `ogexplain analyze` 时提示解析失败。

```
Error: Failed to parse EXPLAIN output
```

**可能原因和解决方案：**

| 原因 | 解决方案 |
|------|---------|
| 输入不是 TEXT 格式的 EXPLAIN 输出 | 确保使用 `EXPLAIN` 或 `EXPLAIN ANALYZE` 的默认 TEXT 输出，而非 JSON、XML 或 YAML 格式 |
| 输入包含非 EXPLAIN 内容 | 使用 `--multi` 启用多块解析，或手动提取纯 EXPLAIN 部分 |
| 输入编码问题 | 确保文件使用 UTF-8 编码 |
| EXPLAIN 输出被截断 | 确保完整的 EXPLAIN 输出，特别是最后的 `Total runtime` 行 |

**调试方法：**

```bash
# 使用 -v 查看详细输出
ogexplain analyze file.txt -v

# 先检查文件内容
head -20 file.txt
```

### 8.2 缺少 db feature

**问题**：使用 `explain` 子命令时提示数据库支持未编译。

```
Error: Database support not compiled. Rebuild with --features db
```

**解决方案：**

```bash
# 重新构建并启用 db feature
cargo build -p ogexplain-cli

# 如果 db feature 被手动禁用，显式启用
cargo build -p ogexplain-cli --features db
```

> **注意：** `db` feature 是默认启用的。如果遇到此错误，可能是自定义了 `Cargo.toml` 中的 default-features。

### 8.3 缺少 mcp feature

**问题**：使用 `mcp` 子命令时提示 MCP 支持未编译。

```
Error: MCP support not compiled. Rebuild with --features mcp
```

**解决方案：**

```bash
# 重新构建并启用 mcp feature
cargo build -p ogexplain-cli --features mcp

# 或者直接使用独立的 MCP 服务器
cargo build -p ogexplain-mcp
ogexplain-mcp
```

### 8.4 编码问题

**问题**：输出中包含乱码或中文显示异常。

**解决方案：**

```bash
# 确保终端使用 UTF-8 编码
export LANG=zh_CN.UTF-8
export LC_ALL=zh_CN.UTF-8

# 使用 --lang 参数明确指定语言
ogexplain analyze file.txt --lang zh-CN

# 检查输入文件编码
file -I file.txt
```

### 8.5 热力图/瀑布图无数据

**问题**：使用 `-o heatmap` 或 `-o waterfall` 时提示无数据或数据不足。

**可能原因**：

- 输入是 `EXPLAIN`（仅估算），而非 `EXPLAIN ANALYZE`（含实际执行统计）
- 热力图和瀑布图需要实际运行时数据

**解决方案：**

```bash
# 确保使用 EXPLAIN ANALYZE 输出
gsql -d mydb -c "EXPLAIN ANALYZE SELECT ..." > analyze_output.txt
ogexplain analyze analyze_output.txt -o heatmap
```

### 8.6 TUI 显示异常

**问题**：TUI 界面显示错乱或按键无响应。

**可能原因和解决方案：**

| 原因 | 解决方案 |
|------|---------|
| 终端尺寸过小 | 将终端窗口调整到至少 80×24 |
| 终端不支持真彩色 | 使用支持真彩色的终端（iTerm2、Windows Terminal、Alacritty 等） |
| 在 tmux/screen 中运行 | 确保 tmux 使用真彩色：`set -g default-terminal "screen-256color"` |

### 8.7 诊断规则未触发

**问题**：预期某条规则应该触发但实际没有。

**可能原因**：

- `--threshold` 过滤级别过高
- 该规则的阈值条件未满足（如 `SCAN-001` 需要表行数超过默认阈值 100,000）
- 输入是纯 `EXPLAIN` 而非 `EXPLAIN ANALYZE`，某些规则依赖实际执行统计

**解决方案：**

```bash
# 确保显示所有级别
ogexplain analyze file.txt --threshold info

# 使用 verbose 模式
ogexplain analyze file.txt -v
```

---

## 附录：25 条诊断规则速查表

| ID | 规则名称 | 类别 | 检测内容 |
|----|---------|------|---------|
| SCAN-001 | 大表全表扫描 | 扫描 | Seq Scan/CStore Scan 超过行数阈值 |
| SCAN-004 | 无索引过滤 | 扫描 | 过滤大量行但缺少索引支持 |
| JOIN-001 | 大表嵌套循环 | 连接 | 两侧行数均较高的 Nested Loop |
| JOIN-002 | Hash 连接磁盘溢出 | 连接 | Hash join 超出 work_mem |
| MEM-001 | 排序磁盘溢出 | 内存 | 外部归并排序（含 VectorSort） |
| MEM-004 | 高峰值内存 | 内存 | 子树中最高内存节点 |
| SORT-003 | 重复排序 | 排序 | 子树中存在重复的 Sort Key |
| NET-001 | 广播大量数据 | 网络 | 跨数据节点广播过多行 |
| EST-001 | 严重行数估算偏差 | 估算 | 实际行数远超/低于优化器估算 |
| EST-004 | 低估导致嵌套循环 | 估算 | 因行数低估导致的 Nested Loop |
| PUSH-001 | 查询未下推 | 下推 | FQS 失败，识别具体阻断因素 |
| PUSH-002 | 多层 Streaming | 下推 | Streaming 层链过长 |
| TYPE-001 | 隐式类型转换 | 类型转换 | 基于间接模式的隐式类型转换检测 |
| TYPE-004 | LIKE 前置通配符 | 类型转换 | LIKE 模式以通配符开头 |
| VEC-001 | 行/向量引擎混用 | 向量化 | Row ↔ Vector 适配器边界 |
| GEN-001 | 执行计划过深 | 通用 | 计划深度过高 |
| SUBQ-001 | 子查询未上拉 | 子查询 | 检测 SubqueryScan 节点 |
| REW-001 | 大 IN 列表未改写 | 子查询 | IN 列表包含大量值 |
| SUBQ-006 | 关联子查询自更新 | 子查询 | UPDATE/DELETE 中的自引用关联子查询 |
| AGG-001 | 聚合应使用 Hash | 聚合 | 大 GROUP BY 应使用 Hash Aggregate |
| AGG-002 | Hash 聚合磁盘溢出 | 聚合 | Hash Aggregate 超出 work_mem |
| SKEW-001 | 数据倾斜 | 分布 | 数据节点间行分布不均 |
| DIST-001 | 分布列不匹配 | 分布 | 连接列与分布列不匹配 |
| STATS-001 | 统计信息未收集 | 统计信息 | 表缺少或统计信息过期 |
| PART-001 | 分区裁剪失败 | 分区 | 全分区扫描 |

---

> **版本说明**：本手册基于 ogexplain-analyzer 当前版本编写。如需查看最新信息，请参考项目仓库中的 README 和 CHANGELOG。
