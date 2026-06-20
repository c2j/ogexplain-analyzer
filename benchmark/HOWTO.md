# Benchmark Pipeline — 完整操作手册

> **前提**: 仓库根目录为 `ogexplain-analyzer/`,下文所有命令均从仓库根目录执行。
> Python 要求 ≥ 3.10(推荐 `~/dev/miniforge3/bin/python`)。

## 流程总览

```
queries_v1.sql  →  [Step 1-2] run_explain.py  →  explains/Q*.explain
                                                      ↓
                         [Step 3] build_cases.py  →  03-build/cases/OGEXP-GT-*.json
                                                      ↓
                         [Step 4] evaluate.py     →  04-evaluate/ (P/R/F1 报告)
```

## Step 1. 设置 query 集(可选 — 仅改动 query 时需要)

```bash
vim lib/ogagila/queries/queries_v1.sql
```

## Step 2. 在 openGauss 容器里跑 EXPLAIN ANALYZE

```bash
# 启动 ogagila 容器(pagila schema)
cd lib/ogagila/ && docker-compose up -d && cd -

# 跑全部 query 并保存 EXPLAIN 输出(脚本在 ogagila 子模块内)
python3 lib/ogagila/queries/run_explain.py \
  --host localhost --port 5432 --db pagila \
  --user gaussdb --password Enmo@123 \
  --out lib/ogagila/queries/explains/
```

输出: `lib/ogagila/queries/explains/Q01.explain` ... `Q97.explain` + 各自 `.meta.json`。

## Step 3. 生成 ground-truth case JSON

```bash
python3 benchmark/03-build/build_cases.py
```

默认从 `lib/ogagila/queries/` 读取,输出到 `benchmark/03-build/cases/`。
生成 97 个 `OGEXP-GT-2026-NNNN.json` + `case_index.json` + `trigger_coverage.md`。

自定义路径:

```bash
python3 benchmark/03-build/build_cases.py \
  --meta /path/to/queries_meta.json \
  --explains-dir /path/to/explains/ \
  --output-dir /path/to/cases/
```

## Step 4. 评估 ogexplain-analyzer

### 4a. Mock 模式(快速验证数据集,不需要编译)

```bash
python3 benchmark/04-evaluate/evaluate.py \
  --mode mock \
  --cases benchmark/03-build/cases/ \
  --output benchmark/04-evaluate/
```

### 4b. Live 模式(真实评估,需要先编译)

```bash
# 先编译 release 版
cargo build --release -p ogexplain-cli

# 跑评估
python3 benchmark/04-evaluate/evaluate.py \
  --mode live \
  --cases benchmark/03-build/cases/ \
  --output benchmark/04-evaluate/ \
  --ogexplain-binary target/release/ogexplain
```

输出文件:

| 文件 | 内容 |
|------|------|
| `evaluation_report.md` | 多维度 P/R/F1 报告(case-level + rule-level + per-severity) |
| `raw_results.json` | 每条 case 的 tool_output vs ground_truth 逐条对照 |
| `confusion_matrix.csv` | TP/FP/FN/TN 按规则展开 |
| `per_rule_metrics.json` | 25 条规则的 P/R/F1 + 命中 case 列表 |
