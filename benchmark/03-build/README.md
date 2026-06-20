# ogagila Ground-Truth Dataset v1

> **Generated:** 2026-06-20
> **Source:** `lib/ogagila/queries/` (openGauss 7.0.0-RC1, pagila schema)
> **Target tool:** ogexplain-analyzer v0.2.x (25 diagnostic rules)
> **Total cases:** 97 (82 problematic + 15 healthy)

## 目录结构

```
03-build/
├── README.md                          ← 本文件
├── build_cases.py                     ← case JSON 生成器
├── ogexplain-groundtruth.schema.json  ← case JSON Schema (Draft 2020-12)
├── case_index.json                    ← 97 case 的索引(evaluator 可直接消费)
├── trigger_coverage.md                ← 按规则维度的触发率报告
└── cases/
    ├── OGEXP-GT-2026-0001.json        ← 97 个 case JSON
    ├── OGEXP-GT-2026-0002.json
    ├── ...
    └── OGEXP-GT-2026-0097.json
```

## 关键事实速览

- **97 条 query** 全部在 ogagila 容器里跑通,2.06 秒完成
- ~53% 触发率:规则设计的触发条件在 EXPLAIN ANALYZE 里真的观察到了对应信号
- 未触发的分两类:
  - 物理上不可能触发(单节点无法触发 DIST-/SKEW-/NET-001)
  - openGauss 跳过了一些副作用语句(DELETE STATISTICS),EST/STATS-* 实质未测试

## 触发率详情

详见 `trigger_coverage.md`(由 `build_cases.py` 自动生成)。

## Case 结构

每个 case JSON 遵循 `ogexplain-groundtruth.schema.json`,包含:

| 字段 | 说明 |
|------|------|
| `input.sql` | 原始 SQL |
| `input.explain_output` | 真 EXPLAIN ANALYZE 输出 |
| `input.plan_actual_runtime_ms` | 实测运行时间(ms) |
| `ground_truth.root_causes[]` | 自动推断的根因(待 DBA 复核) |
| `ground_truth.suggested_fixes[]` | 默认修复模板 |
| `_auto_eval` | 自动评估的规则触发情况(原创字段,方便复核) |

查看单个 case:

```bash
cat cases/OGEXP-GT-2026-0001.json | python3 -m json.tool
```

## 重新生成

```bash
# 从仓库根目录
python3 benchmark/03-build/build_cases.py
```

数据源: `lib/ogagila/queries/explains/Q*.explain` + `lib/ogagila/queries/queries_meta.json`

## 关键 caveat

1. **同作者偏差**: ogagila 与 ogexplain-analyzer 都是 c2j 维护。评估结果可能偏乐观。
2. **openGauss 7.0.0-RC1 不是 LTS**: 生产多用 5/6 版本,执行计划细节可能不同。
3. **数据规模偏小**: payment/rental 各 ~16K 行,阈值型规则即使触发也不会很严重。
4. **9 条 EST/STATS query 的副作用被跳过**: openGauss 不支持 DELETE STATISTICS 语法。

## 已知局限

- `_auto_eval.actually_triggered` 是启发式判断,不是 ground truth。DBA 复核时以此字段不一致则以 DBA 为准。
- `trigger_coverage.md` 中 "Skipped" 列实际包含了两类:物理上没法触发 + 检测器找不到信号。
