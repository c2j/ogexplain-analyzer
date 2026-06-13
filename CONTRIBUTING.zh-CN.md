# 参与贡献

感谢你对 ogexplain-analyzer 的关注！本文档介绍开发工作流、编码规范和如何添加新功能。

## 开发环境搭建

```bash
git clone https://github.com/c2j/ogexplain-analyzer.git
cd ogexplain-analyzer
cargo build --workspace
cargo test --workspace
```

**环境要求：** Rust 2021 edition、Cargo。

**推荐工具：**
- `cargo fmt` — 代码格式化（强制）
- `cargo clippy` — 代码检查（要求零警告）
- `cargo insta` — 快照测试审查

## 项目结构

```
ogexplain-analyzer/
├── crates/
│   ├── ogexplain-core/      # 核心库：解析器、数据模型、分析引擎、建议引擎、SQL改写
│   ├── ogexplain-cli/       # CLI 二进制：analyze、explain、mcp 子命令
│   ├── ogexplain-tui/       # TUI 二进制：交互式计划浏览器
│   ├── ogexplain-mcp/       # MCP 服务器：AI 助手集成
│   └── ogsql-complexity/    # SQL 复杂度评分库
├── tests/
│   ├── fixtures/            # 31 个 EXPLAIN TEXT 测试用例
│   ├── integration_tests.rs # 解析器快照测试
│   └── analyzer_tests.rs    # 诊断规则测试
└── docs/                    # 文档
    └── plans/               # 功能设计方案
```

## 开发流程

### 1. 选择任务/功能

查看[实施方案](.sisyphus/plans/ogexplain-analyzer-impl.md)了解待完成的工作（Phase 4：剩余 20+ 条诊断规则、Markdown 输出、TOML 配置等）。

### 2. 创建分支

```bash
git checkout -b feat/我的功能
# 或
git checkout -b fix/我的修复
```

### 3. 先写测试

**解析器变更**：在 `tests/fixtures/` 中添加测试文件，在 `tests/integration_tests.rs` 中添加 `insta::assert_yaml_snapshot!` 测试。

**新诊断规则**：编写正面测试（应触发规则的执行计划）和负面测试（不应触发规则的执行计划）。

**示例：**

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

### 4. 实现

遵循现有代码模式（见下文各节）。

### 5. 验证

```bash
cargo fmt --all
cargo clippy --workspace          # 必须零警告
cargo test --workspace            # 所有测试必须通过
cargo test --test integration_tests
cargo test --test analyzer_tests
```

### 6. 提交 PR

- PR 标题：使用 `feat:` / `fix:` / `docs:` / `refactor:` 前缀
- 描述：改了什么、为什么、如何验证
- 如有相关问题请关联 Issue

## 编码规范

### Rust 约定

- 所有代码必须通过 `cargo fmt` 格式化（不可协商）
- `cargo clippy --workspace` 必须 **零警告**
- 库代码中禁止使用 `unwrap()` — 使用 `Result` 配合 `thiserror`
- 禁止使用 `as any`、`@ts-ignore` 等类型抑制手段
- 禁止无 `// SAFETY:` 注释的 `unsafe` 块
- 公开 API 必须有文档注释（`cargo doc` 无警告输出）

### 模块组织

- 单个 `.rs` 文件 ≤ 600 行（理想 ≤ 400 行）
- `core` 层必须零 IO/UI 依赖
- `pub(crate)` 仅在必要时使用
- 遵循现有模块布局约定

### 命名规范

- 统一使用 `动词_名词` 风格（如 `parse_text`、`build_tree`）
- getter 方法禁止 `get_` 前缀（用 `name()` 而非 `get_name()`）
- 类型转换：`as_`（借用）、`to_`（可能分配内存）、`into_`（消耗所有权）

## 添加诊断规则

### 步骤 1：理解规则 Trait

```rust
pub trait DiagnosticRule: Send + Sync {
    fn id(&self) -> &str;
    fn name(&self) -> &str;
    fn severity(&self) -> Severity;
    fn category(&self) -> DiagnosticCategory;

    /// 检查单个计划节点。如触发则返回 Some(Finding)。
    fn check(&self, node: &PlanNode, ctx: &PlanContext) -> Option<Finding>;

    /// 跨整个计划做全局检查。返回 Vec<Finding>。
    fn check_global(&self, plan: &ExplainPlan, stats: &GlobalStats) -> Vec<Finding>;
}
```

### 步骤 2：创建规则文件

在 `crates/ogexplain-core/src/analyzer/rules/` 中创建新文件（如 `my_rules.rs`）。

### 步骤 3：实现规则

```rust
pub struct MyNewRule;

impl DiagnosticRule for MyNewRule {
    fn id(&self) -> &str { "CAT-001" }

    fn name(&self) -> &str { "我的新规则" }

    fn severity(&self) -> Severity { Severity::Warning }

    fn category(&self) -> DiagnosticCategory { DiagnosticCategory::General }

    fn check(&self, node: &PlanNode, ctx: &PlanContext) -> Option<Finding> {
        // 仅对特定节点类型触发
        if node.node_type != NodeType::SeqScan {
            return None;
        }

        // 检查条件
        let estimated_rows = node.estimated.as_ref()?.plan_rows;
        if estimated_rows < 10000.0 {
            return None;
        }

        // 构建诊断发现
        Some(Finding {
            rule_id: self.id().to_string(),
            severity: self.severity(),
            category: self.category(),
            title: self.name().to_string(),
            detail: format!("表 {} 有 {} 估算行数", table_name, estimated_rows),
            node_line: Some(node.line_number),
            node_type: Some(node.node_type.to_string()),
            suggestion: Some("建议添加索引".to_string()),
            sql_rewrite: None,
            evidence: None,
        })
    }

    fn check_global(&self, _plan: &ExplainPlan, _stats: &GlobalStats) -> Vec<Finding> {
        vec![]
    }
}
```

### 步骤 4：注册规则

在 `crates/ogexplain-core/src/analyzer/rules/mod.rs` 中，将规则添加到 `all_rules()` 函数：

```rust
pub fn all_rules(config: &DiagnosticConfig) -> Vec<Box<dyn DiagnosticRule>> {
    let mut rules: Vec<Box<dyn DiagnosticRule>> = vec![
        Box::new(scan_rules::LargeTableFullScan::default()),
        // ... 已有规则 ...
        Box::new(my_rules::MyNewRule),  // 在此添加
    ];
    rules.retain(|r| !config.disabled_rules.contains(&r.id().to_string()));
    rules
}
```

### 步骤 5：编写测试

在 `tests/analyzer_tests.rs` 中添加正面和负面测试。如需要可创建测试用例文件。

### 步骤 6：添加规则元数据

更新 `crates/ogexplain-mcp/src/server.rs` 中 `list_diagnostic_rules` 工具的规则列表。

## 测试指南

### 解析器测试（integration_tests.rs）

- 每个新的节点类型或解析功能都需要一个测试用例 + 快照测试
- 使用 `insta::assert_yaml_snapshot!` 进行回归测试
- 用 `cargo insta review` 审查快照

### 诊断规则测试（analyzer_tests.rs）

- **正面测试**：规则在合适的执行计划上触发 → 断言发现存在
- **负面测试**：规则在无关的执行计划上不触发 → 断言发现不存在
- 测试边界情况：空计划、单节点、深度嵌套、缺失统计信息

### 运行特定测试

```bash
cargo test -p ogexplain-core -- test_my_new_rule
cargo test --test analyzer_tests -- my_new_rule
cargo test --test integration_tests -- my_fixture
```

## 提交规范

使用约定式提交前缀：

- `feat:` — 新功能（诊断规则、输出格式、子命令）
- `fix:` — Bug 修复
- `docs:` — 文档变更
- `test:` — 添加或更新测试
- `refactor:` — 代码重构（无行为变化）
- `chore:` — 构建、CI、依赖更新

示例：
```
feat: 添加 SCAN-005 位图扫描检测规则

检测小表上 Bitmap Heap Scan 应改为普通 Seq Scan 的情况。
包含正面和负面测试。
```

## 代码审查清单

提交 PR 前，请确认：

- [ ] `cargo fmt --all` 通过
- [ ] `cargo clippy --workspace` — 零警告
- [ ] `cargo test --workspace` — 所有测试通过
- [ ] 库代码中无 `unwrap()`
- [ ] 无类型抑制（`as any`、`@ts-ignore`）
- [ ] 新增公开项有文档注释
- [ ] 新诊断规则有正面和负面测试
- [ ] MCP 服务器的 `list_diagnostic_rules` 中已更新规则元数据
- [ ] 无遗留的注释代码

## 有疑问？

- 查阅 `.sisyphus/plans/ogexplain-analyzer-spec.md` 了解详细设计规格
- 查阅 `.sisyphus/plans/ogexplain-analyzer-impl.md` 了解实施方案和待完成工作
- 查阅 `AGENTS.md` 了解 AI 助手使用指南
- 参考 `docs/CONTRIBUTING.md` 了解强制 Rust 编码标准
- 参考 `docs/BEST-PRATICE.md` 了解推荐 Rust 最佳实践
