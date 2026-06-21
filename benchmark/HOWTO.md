# Benchmark Evaluation — 操作手册

> **前提**:仓库根目录为 `ogexplain-analyzer/`,下文所有命令均从仓库根执行。
> Python 要求 ≥ 3.10。

## 角色分工

| 仓库 | 职责 |
|------|------|
| `ogagila` (submodule) | **数据生成** — SQL 查询集、EXPLAIN ANALYZE 输出、ground-truth case JSON |
| `ogexplain-analyzer` (本仓库) | **评估消费** — `evaluate.py` 跑准确率评估,产出 P/R/F1 报告 |

完整数据集生成流程(SQL → EXPLAIN → case JSON)见 [`lib/ogagila/benchmark/README.md`](../lib/ogagila/benchmark/README.md)。

## 流程总览

```
lib/ogagila/benchmark/v1/cases/OGEXP-GT-*.json   ← ground-truth 数据源(由 ogagila 生成)
                       │
                       ▼
              benchmark/04-evaluate/evaluate.py
                       │
                       ▼
              benchmark/04-evaluate/live_results[_vX.Y.Z]/   ← 版本化评估结果
```

## 前置:初始化 ogagila 子模块

```bash
git submodule update --init lib/ogagila
```

未初始化时 `lib/ogagila/benchmark/v1/cases/` 不存在,评估会因找不到 case 而失败。

## 评估 ogexplain-analyzer

### Mock 模式(快速验证数据集,不需要编译)

```bash
python3 benchmark/04-evaluate/evaluate.py \
  --mode mock \
  --cases lib/ogagila/benchmark/v1/cases/ \
  --output benchmark/04-evaluate/
```

用 case 里的 `_auto_eval` 字段叠加噪声作为"工具输出",适合 demo / CI / 数据集完整性检查。

### Live 模式(真实评估,需要先编译)

```bash
# 先编译 release 版
cargo build --release -p ogexplain-cli

# 跑评估
python3 benchmark/04-evaluate/evaluate.py \
  --mode live \
  --cases lib/ogagila/benchmark/v1/cases/ \
  --output benchmark/04-evaluate/ \
  --ogexplain-binary target/release/ogexplain
```

## 版本化结果留存

每次发布版本跑一轮 live 评估,结果落在独立目录:

```
benchmark/04-evaluate/
├── evaluate.py
├── live_results/             ← 最新一轮(soft link 或复制)
├── live_results_v0.3.0/      ← v0.3.0 时评估结果
└── live_results_v0.3.1/      ← v0.3.1 时评估结果
```

跑指定版本的评估时,把 `--output` 指向新目录即可:

```bash
python3 benchmark/04-evaluate/evaluate.py \
  --mode live \
  --cases lib/ogagila/benchmark/v1/cases/ \
  --output benchmark/04-evaluate/live_results_v0.4.0/ \
  --ogexplain-binary target/release/ogexplain
```

## 输出文件

每个评估目录包含:

| 文件 | 内容 |
|------|------|
| `evaluation_report.md` | 多维度 P/R/F1 报告(case-level + rule-level + per-severity) |
| `raw_results.json` | 每条 case 的 tool_output vs ground_truth 逐条对照 |
| `confusion_matrix.csv` | TP/FP/FN/TN 按规则展开 |
| `per_rule_metrics.json` | 25 条规则的 P/R/F1 + 命中 case 列表 |

## 重新生成 ground-truth 数据集

数据集的更新流程(SQL 调整、重新跑 EXPLAIN、重建 case JSON)全部在 ogagila 子模块内完成,见 [`lib/ogagila/benchmark/README.md`](../lib/ogagila/benchmark/README.md) 的 "Stage A / Stage B" 章节。本仓库不再保留任何生成器副本。
