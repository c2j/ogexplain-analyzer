#!/usr/bin/env python3
"""
evaluate.py — 评估 ogexplain-analyzer 对 ground-truth case 的诊断准确率。

两种运行模式:
  --mode mock    用 case 里的 _auto_eval 字段作为"工具输出"(适合调试、demo、CI)
  --mode live    调 ogexplain CLI 跑真诊断(需要先 cargo build --release)

输出:
  evaluation_report.md — 多维度 P/R/F1 报告
  raw_results.json    — 每条 case 的 tool_output vs ground_truth 对照
  confusion_matrix.csv — TP/FP/FN/TN 按规则展开

前置:ground-truth case 由 ogagila 子模块提供,需先初始化:
  git submodule update --init lib/ogagila

用法(从仓库根调用):

  # Mock 模式(不需要编译 ogexplain,用于快速验证数据集)
  python3 benchmark/04-evaluate/evaluate.py --mode mock \\
    --cases lib/ogagila/benchmark/v1/cases/ \\
    --output benchmark/04-evaluate/

  # Live 模式(真实评估)
  python3 benchmark/04-evaluate/evaluate.py --mode live \\
    --cases lib/ogagila/benchmark/v1/cases/ \\
    --output benchmark/04-evaluate/ \\
    --ogexplain-binary target/release/ogexplain
"""
from __future__ import annotations

import argparse
import csv
import json
import subprocess
import sys
from collections import defaultdict
from pathlib import Path
from statistics import mean, median
from datetime import datetime, timezone

# ------------------------------------------------------------------
# 数据加载
# ------------------------------------------------------------------

def load_cases(cases_dir: Path) -> list[dict]:
    cases = []
    for p in sorted(cases_dir.glob('OGEXP-*.json')):
        try:
            cases.append(json.loads(p.read_text(encoding='utf-8')))
        except Exception as e:
            print(f"[WARN] skip {p.name}: {e}", file=sys.stderr)
    return cases


# ------------------------------------------------------------------
# 工具输出获取:mock 或 live
# ------------------------------------------------------------------

def get_tool_output_mock(case: dict) -> dict:
    """
    Mock 模式:用 build_cases.py 留下的 _auto_eval 当作"工具输出"，
    但叠加一些随机误差以模拟真实工具的不完美表现。
    可以使用 --mock-noise 调整误差率。
    """
    import random
    auto = case.get('_auto_eval', {})
    designed_rule = auto.get('target_rule_designed', '')
    actually_triggered = auto.get('actually_triggered', False)
    is_healthy = case.get('ground_truth', {}).get('is_problematic') is False
    warnings = auto.get('run_warnings', [])

    noise_level = getattr(get_tool_output_mock, '_noise', 0.0)
    seed = hash(case['case_id']) % (2**32)
    rng = random.Random(seed)

    if is_healthy:
        # 健康 case:小概率误报
        if rng.random() < noise_level * 0.5:
            return {'findings': [{
                'rule_id': 'SCAN-001',
                'severity': 'warning',
                'category': 'SCAN-',
                'detail': 'mock: false positive on healthy case',
                'source': 'mock:noisy',
            }], 'source': 'mock'}
        return {'findings': [], 'source': 'mock'}

    if designed_rule == 'NONE' or not designed_rule:
        return {'findings': [], 'source': 'mock'}

    if warnings:
        # 语句被跳过:工具拿不到信号,通常报不出
        return {'findings': [], 'source': 'mock'}

    if not actually_triggered:
        # 设计了但没真的触发:工具有概率报不出
        if rng.random() < noise_level * 0.7:
            return {'findings': [], 'source': 'mock:noisy:missed_weak_signal'}

    findings = [{
        'rule_id': designed_rule,
        'severity': case.get('ground_truth', {}).get('expected_severity_if_problematic', 'warning'),
        'category': case.get('ground_truth', {}).get('root_causes', [{}])[0].get('ogexplain_rule_category', ''),
        'detail': auto.get('trigger_signal', ''),
        'source': 'mock:_auto_eval',
    }]

    # 偶尔额外多报一个(模拟 FP)
    if rng.random() < noise_level * 0.4:
        extra_rules = ['SCAN-004', 'JOIN-001', 'MEM-001', 'PUSH-001']
        extra = rng.choice(extra_rules)
        if extra != designed_rule:
            findings.append({
                'rule_id': extra,
                'severity': 'warning',
                'category': extra.split('-')[0] + '-',
                'detail': 'mock: extra false-positive',
                'source': 'mock:noisy:extra',
            })

    return {'findings': findings, 'source': 'mock'}


def get_tool_output_live(case: dict, ogexplain_binary: str) -> dict:
    """
    Live 模式:写 explain_output 到临时文件,调 ogexplain CLI,解析 JSON 输出。
    """
    explain_text = case['input']['explain_output']
    # 写临时文件
    tmp_path = Path('/tmp') / f"ogexplain_{case['case_id']}.txt"
    tmp_path.write_text(explain_text, encoding='utf-8')

    try:
        result = subprocess.run(
            [ogexplain_binary, 'analyze', str(tmp_path), '-o', 'json'],
            capture_output=True, text=True, timeout=30,
        )
        if result.returncode != 0:
            return {'findings': [], 'source': 'live', 'error': result.stderr.strip()[:200]}

        stdout = result.stdout
        findings = []

        # ogexplain 在输入含 SET 语句时输出多块格式:
        #   ═══ 第 1/2 块（仅SQL） ═══ {...}
        #   ═══ 第 2/2 块 ═══ {...}
        # 需要切分后逐块解析,聚合所有块的 findings。
        if '═══' in stdout:
            import re
            blocks = re.split(r'═{3,}\s*第\s*\d+/\d+\s*块[^═]*═{3,}', stdout)
            for block in blocks:
                block = block.strip()
                if not block:
                    continue
                try:
                    bdata = json.loads(block)
                    findings.extend(bdata.get('findings', bdata.get('results', [])))
                except json.JSONDecodeError:
                    pass
        else:
            try:
                data = json.loads(stdout)
                findings = data.get('findings', data.get('results', []))
            except json.JSONDecodeError:
                return {'findings': [], 'source': 'live', 'error': 'JSON parse failed'}

        # 标准化
        norm = []
        for f in findings:
            norm.append({
                'rule_id': f.get('rule_id') or f.get('id') or '',
                'severity': f.get('severity', 'warning'),
                'category': f.get('category', ''),
                'detail': f.get('detail', '')[:200],
            })
        return {'findings': norm, 'source': 'live'}
    finally:
        try:
            tmp_path.unlink()
        except FileNotFoundError:
            pass


# ------------------------------------------------------------------
# 环境检测:单节点 centralized 模式下部分规则物理不可触发
# ------------------------------------------------------------------

ENV_UNREACHABLE_MAP = {
    'Streaming': ['DIST-001', 'SKEW-001', 'PUSH-001', 'PUSH-002', 'NET-001'],
    'Adapter': ['VEC-001'],
}


def detect_env_unreachable(explain_text: str) -> set[str]:
    plan_lines = ' '.join(l for l in explain_text.split('\n') if '(cost=' in l)
    unreachable = set()
    for signal, rules in ENV_UNREACHABLE_MAP.items():
        if signal not in plan_lines:
            unreachable.update(rules)
    return unreachable


# ------------------------------------------------------------------
# 匹配与统计
# ------------------------------------------------------------------

def extract_expected_rules(case: dict) -> list[str]:
    """从 ground_truth.root_causes 取 ogexplain_rule_id"""
    rules = []
    for rc in case.get('ground_truth', {}).get('root_causes', []):
        rid = rc.get('ogexplain_rule_id')
        if rid and rid != 'NONE':
            rules.append(rid)
    return rules


def classify_case(case: dict, tool_output: dict) -> dict:
    """
    对一条 case 做 TP/FP/FN/TN 分类。
    返回: {
        'expected_rules': [...],
        'reported_rules': [...],
        'tp': [...], 'fp': [...], 'fn': [...],
        'is_healthy': bool,
    }
    """
    expected = extract_expected_rules(case)
    is_healthy = case.get('ground_truth', {}).get('is_problematic') is False
    reported = [f.get('rule_id', '') for f in tool_output.get('findings', [])]

    # 健康 case 的逻辑:工具不应该报任何东西
    if is_healthy:
        if not reported:
            return {
                'case_id': case['case_id'],
                'is_healthy': True,
                'expected_rules': [],
                'reported_rules': reported,
                'tp': [],
                'fp': [],
                'fn': [],
                'env_skipped': [],
                'verdict': 'TN',
            }
        else:
            return {
                'case_id': case['case_id'],
                'is_healthy': True,
                'expected_rules': [],
                'reported_rules': reported,
                'tp': [],
                'fp': reported,
                'fn': [],
                'env_skipped': [],
                'verdict': 'FP',
            }

    # 非健康 case:按规则匹配
    expected_set = set(expected)
    reported_set = set(reported)
    tp = list(expected_set & reported_set)
    fp = list(reported_set - expected_set)
    fn = list(expected_set - reported_set)

    env_unreachable = detect_env_unreachable(case.get('input', {}).get('explain_output', ''))
    env_skipped = [r for r in fn if r in env_unreachable]
    fn = [r for r in fn if r not in env_unreachable]

    if not expected_set:
        # GT 没有规则但 case 非健康(异常,理论不应发生)
        verdict = 'EMPTY_GT'
    elif tp and not fn and not fp:
        verdict = 'TP'
    elif tp and not fn:
        verdict = 'TP+FP'  # 命中但多报
    elif tp and not fp:
        verdict = 'TP+FN'  # 部分命中,漏报其他
    elif not tp and fp:
        verdict = 'FP'  # 误报
    elif not tp and not fp:
        verdict = 'FN'  # 完全漏报
    else:
        verdict = 'MIXED'

    return {
        'case_id': case['case_id'],
        'is_healthy': False,
        'expected_rules': expected,
        'reported_rules': reported,
        'tp': tp,
        'fp': fp,
        'fn': fn,
        'env_skipped': env_skipped,
        'verdict': verdict,
    }


def compute_metrics(tp: int, fp: int, fn: int) -> dict:
    precision = tp / (tp + fp) if (tp + fp) > 0 else 0.0
    recall = tp / (tp + fn) if (tp + fn) > 0 else 0.0
    f1 = 2 * precision * recall / (precision + recall) if (precision + recall) > 0 else 0.0
    return {
        'tp': tp,
        'fp': fp,
        'fn': fn,
        'precision': precision,
        'recall': recall,
        'f1': f1,
    }


# ------------------------------------------------------------------
# 报告生成
# ------------------------------------------------------------------

def build_report(classifications: list[dict], cases: list[dict]) -> dict:
    """聚合各种分层指标"""
    # 总数
    total = len(classifications)
    n_healthy = sum(1 for c in classifications if c['is_healthy'])
    n_problematic = total - n_healthy

    # 全局 TP/FP/FN(按"是否有任何匹配"算)
    # 这里用"case 级别"判断:case 至少一条 rule 命中算 TP-ish,反之 FN-ish
    case_tp = case_fp = case_fn = case_tn = 0
    rule_tp = rule_fp = rule_fn = 0

    for cls in classifications:
        if cls['is_healthy']:
            if not cls['reported_rules']:
                case_tn += 1
            else:
                case_fp += 1
        else:
            if cls['tp'] and not cls['fn']:
                case_tp += 1
            elif cls['fn'] and not cls['tp']:
                case_fn += 1
            else:
                # partial
                if cls['tp']:
                    case_tp += 1  # 至少命中一个,算 partial TP
                else:
                    case_fn += 1
        rule_tp += len(cls['tp'])
        rule_fp += len(cls['fp'])
        rule_fn += len(cls['fn'])

    # 按规则维度
    per_rule = defaultdict(lambda: {'tp': 0, 'fp': 0, 'fn': 0, 'cases': []})
    for cls in classifications:
        for r in cls['tp']:
            per_rule[r]['tp'] += 1
            per_rule[r]['cases'].append(cls['case_id'])
        for r in cls['fp']:
            per_rule[r]['fp'] += 1
        for r in cls['fn']:
            per_rule[r]['fn'] += 1

    # 按严重级
    per_severity = defaultdict(lambda: {'tp': 0, 'fp': 0, 'fn': 0})
    case_by_id = {c['case_id']: c for c in cases}
    for cls in classifications:
        for r in cls['tp'] + cls['fn']:
            case = case_by_id.get(cls['case_id'])
            if not case:
                continue
            sev = case.get('ground_truth', {}).get('expected_severity_if_problematic', 'unknown')
            if r in cls['tp']:
                per_severity[sev]['tp'] += 1
            else:
                per_severity[sev]['fn'] += 1
        for r in cls['fp']:
            # FP 的严重级从 reported 推断(暂用 warning 占位)
            per_severity['warning']['fp'] += 1

    return {
        'total_cases': total,
        'n_healthy': n_healthy,
        'n_problematic': n_problematic,
        'case_level': {
            'tp': case_tp, 'fp': case_fp, 'fn': case_fn, 'tn': case_tn,
            **compute_metrics(case_tp, case_fp, case_fn),
        },
        'rule_level': {
            'tp': rule_tp, 'fp': rule_fp, 'fn': rule_fn,
            **compute_metrics(rule_tp, rule_fp, rule_fn),
        },
        'per_rule': {r: {**compute_metrics(d['tp'], d['fp'], d['fn']), 'cases': d['cases']}
                     for r, d in per_rule.items()},
        'per_severity': {s: compute_metrics(d['tp'], d['fp'], d['fn'])
                         for s, d in per_severity.items()},
    }


def render_markdown_report(report: dict, classifications: list[dict],
                            cases: list[dict], mode: str) -> str:
    """生成 markdown 报告"""
    lines = []
    lines += [
        '# ogexplain-analyzer Diagnostic Accuracy Report',
        '',
        f'**Generated:** {datetime.now(timezone.utc).isoformat()}',
        f'**Mode:** {mode}',
        f'**Cases:** {report["total_cases"]} ({report["n_problematic"]} problematic + {report["n_healthy"]} healthy)',
        '',
        '---',
        '',
        '## 1. Summary',
        '',
    ]

    # Case-level
    cl = report['case_level']
    lines += [
        '### Case-level (overall)',
        '',
        '每个 case 看是否有"命中"(至少一条规则报对了)。',
        '',
        f'| Metric | Value |',
        f'|--------|-------|',
        f'| TP | {cl["tp"]} |',
        f'| FP | {cl["fp"]} |',
        f'| FN | {cl["fn"]} |',
        f'| TN | {cl["tn"]} |',
        f'| **Precision** | **{cl["precision"]:.1%}** |',
        f'| **Recall** | **{cl["recall"]:.1%}** |',
        f'| **F1** | **{cl["f1"]:.1%}** |',
        '',
    ]

    # Rule-level
    rl = report['rule_level']
    lines += [
        '### Rule-level (strict)',
        '',
        '每条规则算 TP/FP/FN。同一 case 多规则按规则分别统计。',
        '',
        f'| Metric | Value |',
        f'|--------|-------|',
        f'| TP | {rl["tp"]} |',
        f'| FP | {rl["fp"]} |',
        f'| FN | {rl["fn"]} |',
        f'| **Precision** | **{rl["precision"]:.1%}** |',
        f'| **Recall** | **{rl["recall"]:.1%}** |',
        f'| **F1** | **{rl["f1"]:.1%}** |',
        '',
    ]

    lines += ['---', '', '## 2. Per-rule breakdown', '']
    lines += [
        '| Rule | TP | FP | FN | Precision | Recall | F1 | Cases |',
        '|------|----|----|----|-----------|--------|-----|-------|',
    ]
    for rule in sorted(report['per_rule'].keys()):
        m = report['per_rule'][rule]
        lines.append(
            f'| {rule} | {m["tp"]} | {m["fp"]} | {m["fn"]} | '
            f'{m["precision"]:.1%} | {m["recall"]:.1%} | {m["f1"]:.1%} | '
            f'{", ".join(m["cases"][:3])}{"..." if len(m["cases"]) > 3 else ""} |'
        )
    lines.append('')

    lines += ['---', '', '## 3. Per-severity breakdown', '']
    lines += [
        '| Severity | TP | FP | FN | Precision | Recall | F1 |',
        '|----------|----|----|----|-----------|--------|-----|',
    ]
    for sev in sorted(report['per_severity'].keys()):
        m = report['per_severity'][sev]
        lines.append(f'| {sev} | {m["tp"]} | {m["fp"]} | {m["fn"]} | '
                     f'{m["precision"]:.1%} | {m["recall"]:.1%} | {m["f1"]:.1%} |')
    lines.append('')

    # FN samples
    lines += ['---', '', '## 4. Notable False Negatives (missed real problems)', '']
    fn_cases = [cls for cls in classifications if cls['fn']]
    if fn_cases:
        lines += ['| Case | Expected | Reported |', '|------|----------|----------|']
        for cls in fn_cases[:10]:
            lines.append(f'| {cls["case_id"]} | {", ".join(cls["expected_rules"])} | '
                         f'{", ".join(cls["reported_rules"]) if cls["reported_rules"] else "(none)"} |')
        if len(fn_cases) > 10:
            lines.append(f'| _...{len(fn_cases) - 10} more_ | | |')
    else:
        lines.append('_None_')
    lines.append('')

    # FP samples
    lines += ['---', '', '## 5. Notable False Positives (over-reported)', '']
    fp_cases = [cls for cls in classifications if cls['fp']]
    if fp_cases:
        lines += ['| Case | Expected | Reported (false alarms) |', '|------|----------|------------------------|']
        for cls in fp_cases[:10]:
            lines.append(f'| {cls["case_id"]} | {", ".join(cls["expected_rules"]) if cls["expected_rules"] else "(healthy)"} | '
                         f'{", ".join(cls["reported_rules"])} |')
        if len(fp_cases) > 10:
            lines.append(f'| _...{len(fp_cases) - 10} more_ | | |')
    else:
        lines.append('_None_')
    lines.append('')

    # Recommendations
    lines += ['---', '', '## 6. Recommendations', '']
    if rl['recall'] < 0.5:
        lines.append('- ⚠️ **Recall 太低** — 工具漏报超过一半的问题,DBA 不能依赖它做告警。建议补充更多针对性训练 case。')
    if rl['precision'] < 0.5:
        lines.append('- ⚠️ **Precision 太低** — 工具误报太多,DBA 会被噪音淹没。建议收紧规则的触发阈值。')
    if rl['f1'] < 0.6:
        lines.append('- ⚠️ **F1 < 0.6** — 工具整体不可靠,需要回到规则定义层面复盘。')

    # 找最弱的 3 条规则
    weak = sorted(report['per_rule'].items(), key=lambda x: x[1]['f1'])[:3]
    if weak:
        lines += ['', '### 最弱规则(按 F1 排序,前 3)']
        for rule, m in weak:
            lines.append(f'- **{rule}** — F1={m["f1"]:.1%} (TP={m["tp"]}, FP={m["fp"]}, FN={m["fn"]})')
        lines += ['', '建议:针对这些规则增加更多 case,看是规则定义本身的问题还是触发条件太严。']

    lines += ['', '---', '', '## 7. How to read this report', '']
    lines += [
        '- **Case-level**:粗粒度,看工具整体能否"识别出问题 case"。',
        '- **Rule-level**:细粒度,看工具每条规则的准确度。',
        '- **Per-rule**:每条规则的 TP/FP/FN,可定位弱规则。',
        '- **Per-severity**:critical 级问题通常更重要,看这个维度上 Recall 是否足够。',
        '- **FN**:漏报,最危险——本应报告但工具没说。',
        '- **FP**:误报,会浪费 DBA 注意力。',
        '',
        f'_Report generated by evaluate.py v1.0, mode={mode}_',
    ]
    return '\n'.join(lines)


def render_confusion_csv(report: dict, classifications: list[dict],
                         cases: list[dict]) -> str:
    """生成 CSV 形式,方便进一步分析"""
    case_by_id = {c['case_id']: c for c in cases}
    rows = []
    rows.append(['case_id', 'is_healthy', 'expected_rules', 'reported_rules',
                'tp', 'fp', 'fn', 'verdict', 'target_severity'])
    for cls in classifications:
        case = case_by_id.get(cls['case_id'], {})
        sev = case.get('ground_truth', {}).get('expected_severity_if_problematic', '')
        rows.append([
            cls['case_id'],
            '1' if cls['is_healthy'] else '0',
            '|'.join(cls['expected_rules']),
            '|'.join(cls['reported_rules']),
            '|'.join(cls['tp']),
            '|'.join(cls['fp']),
            '|'.join(cls['fn']),
            cls['verdict'],
            sev or '',
        ])
    output = []
    for row in rows:
        output.append(','.join(f'"{c}"' for c in row))
    return '\n'.join(output)


# ------------------------------------------------------------------
# Main
# ------------------------------------------------------------------

def main():
    p = argparse.ArgumentParser(description='Evaluate ogexplain-analyzer accuracy on ground-truth dataset')
    p.add_argument('--cases', required=True, help='Path to cases directory (OGEXP-*.json)')
    p.add_argument('--output', required=True, help='Output directory')
    p.add_argument('--mode', choices=['mock', 'live'], default='mock',
                   help='mock: use _auto_eval field; live: invoke ogexplain CLI')
    p.add_argument('--ogexplain-binary', default='ogexplain',
                   help='Path to ogexplain binary (for live mode)')
    p.add_argument('--mock-noise', type=float, default=0.2,
                   help='Mock 模式噪声率(0-1),默认 0.2')
    p.add_argument('--limit', type=int, default=None,
                   help='Limit number of cases (for quick smoke test)')
    args = p.parse_args()

    cases_dir = Path(args.cases)
    output_dir = Path(args.output)
    output_dir.mkdir(parents=True, exist_ok=True)

    cases = load_cases(cases_dir)
    if args.limit:
        cases = cases[:args.limit]
    print(f"[load] {len(cases)} cases from {cases_dir}", file=sys.stderr)

    skipped_ids = []
    filtered = []
    for case in cases:
        run_warnings = case.get('_auto_eval', {}).get('run_warnings', [])
        if run_warnings:
            skipped_ids.append(case['case_id'])
        else:
            filtered.append(case)
    if skipped_ids:
        cases = filtered
        print(f"[skip] {len(skipped_ids)} cases excluded (run_warnings): {skipped_ids}", file=sys.stderr)

    if args.mode == 'mock':
        get_tool_output_mock._noise = args.mock_noise
        print(f"[mock-noise] {args.mock_noise}", file=sys.stderr)

    # 获取工具输出并分类
    classifications = []
    raw_results = []
    for case in cases:
        try:
            if args.mode == 'mock':
                tool_output = get_tool_output_mock(case)
            else:
                tool_output = get_tool_output_live(case, args.ogexplain_binary)
        except Exception as e:
            print(f"  [ERR] {case['case_id']}: {e}", file=sys.stderr)
            tool_output = {'findings': [], 'source': args.mode, 'error': str(e)}

        cls = classify_case(case, tool_output)
        classifications.append(cls)
        raw_results.append({
            'case_id': case['case_id'],
            'tool_output': tool_output,
            'classification': cls,
        })

    # 汇总
    report = build_report(classifications, cases)
    env_skipped_total = sum(len(cls.get('env_skipped', [])) for cls in classifications)
    print(f"[report] {len(classifications)} classifications, "
          f"case-F1={report['case_level']['f1']:.1%}, rule-F1={report['rule_level']['f1']:.1%}",
          file=sys.stderr)
    if env_skipped_total > 0:
        print(f"[env] {env_skipped_total} FN excluded as environmentally unreachable "
              f"(no Streaming/Adapter nodes in EXPLAIN)", file=sys.stderr)

    # 输出报告
    md = render_markdown_report(report, classifications, cases, args.mode)
    (output_dir / 'evaluation_report.md').write_text(md, encoding='utf-8')

    # 输出 raw_results.json
    (output_dir / 'raw_results.json').write_text(
        json.dumps({
            'generated_at': datetime.now(timezone.utc).isoformat(),
            'mode': args.mode,
            'summary': {
                'case_level': report['case_level'],
                'rule_level': report['rule_level'],
                'per_rule_count': len(report['per_rule']),
            },
            'results': raw_results,
        }, indent=2, ensure_ascii=False),
        encoding='utf-8',
    )

    # 输出 confusion_matrix.csv
    (output_dir / 'confusion_matrix.csv').write_text(
        render_confusion_csv(report, classifications, cases),
        encoding='utf-8',
    )

    # 输出 per_rule metrics
    (output_dir / 'per_rule_metrics.json').write_text(
        json.dumps(report['per_rule'], indent=2, ensure_ascii=False),
        encoding='utf-8',
    )

    print(f"\n[done] 输出:", file=sys.stderr)
    print(f"  {output_dir}/evaluation_report.md", file=sys.stderr)
    print(f"  {output_dir}/raw_results.json", file=sys.stderr)
    print(f"  {output_dir}/confusion_matrix.csv", file=sys.stderr)
    print(f"  {output_dir}/per_rule_metrics.json", file=sys.stderr)


if __name__ == '__main__':
    main()