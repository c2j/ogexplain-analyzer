#!/usr/bin/env python3
"""
build_cases.py — 把 ogagila 跑出来的 97 份 EXPLAIN ANALYZE 转成 ground-truth case JSON。

输入:
  <repo>/lib/ogagila/queries/queries_meta.json   — query 元数据
  <repo>/lib/ogagila/queries/explains/Q*.explain — 真 EXPLAIN ANALYZE 输出
  <repo>/lib/ogagila/queries/explains/Q*.meta.json — 运行时元数据(含 warnings)

输出:
  benchmark/03-build/cases/OGEXP-GT-2026-NNNN.json
  benchmark/03-build/case_index.json
  benchmark/03-build/trigger_coverage.md

用法:
  # 从仓库根目录
  python3 benchmark/03-build/build_cases.py

  # 自定义路径
  python3 benchmark/03-build/build_cases.py \
      --meta /path/to/queries_meta.json \
      --explains-dir /path/to/explains/ \
      --output-dir /path/to/cases/
"""
from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path
from datetime import datetime, timezone

# 默认路径(相对于脚本位置)
SCRIPT_DIR = Path(__file__).resolve().parent          # benchmark/03-build/
REPO_ROOT = SCRIPT_DIR.parents[1]                      # ogexplain-analyzer/
DEFAULT_META = REPO_ROOT / 'lib' / 'ogagila' / 'queries' / 'queries_meta.json'
DEFAULT_EXPLAINS = REPO_ROOT / 'lib' / 'ogagila' / 'queries' / 'explains'
DEFAULT_OUTPUT = SCRIPT_DIR / 'cases'

# 规则类别 → ogexplain_rule_category 映射
RULE_PREFIX_TO_CATEGORY = {
    'SCAN': 'SCAN-', 'JOIN': 'JOIN-', 'MEM': 'MEM-', 'SORT': 'SORT-',
    'NET': 'NET-', 'EST': 'EST-', 'PUSH': 'PUSH-', 'TYPE': 'TYPE-',
    'VEC': 'VEC-', 'SUBQ': 'SUBQ-', 'AGG': 'AGG-', 'DIST': 'DIST-',
    'STATS': 'STAT-', 'PART': 'PART-', 'GEN': 'GEN-', 'SKEW': 'DIST-',
    'REW': 'SUBQ-',
}

# 规则 → 默认 root cause category 枚举(对齐 ogexplain-groundtruth.schema.json)
RULE_TO_CAUSE_CATEGORY = {
    'SCAN-001': 'no_real_problem',  # placeholder, 大表 Seq Scan 本身是问题
    'SCAN-004': 'missing_index',
    'JOIN-001': 'suboptimal_join_algorithm',
    'JOIN-002': 'memory_pressure',
    'MEM-001': 'excessive_sort_spill',
    'MEM-004': 'memory_pressure',
    'SORT-003': 'no_real_problem',  # duplicate sort, 占位
    'NET-001': 'broadcast_skew',
    'EST-001': 'over_under_estimation',
    'EST-004': 'over_under_estimation',
    'TYPE-001': 'implicit_type_coercion',
    'TYPE-004': 'no_real_problem',  # LIKE wildcard, 占位
    'PUSH-001': 'pushdown_missed',
    'PUSH-002': 'pushdown_missed',
    'PART-001': 'partition_pruning_failed',
    'SUBQ-001': 'subquery_inefficiency',
    'SUBQ-006': 'subquery_inefficiency',
    'REW-001': 'no_real_problem',
    'VEC-001': 'vectorization_disabled',
    'AGG-001': 'no_real_problem',
    'AGG-002': 'memory_pressure',
    'DIST-001': 'no_real_problem',
    'SKEW-001': 'broadcast_skew',
    'STATS-001': 'stale_statistics',
    'GEN-001': 'no_real_problem',
}

# 默认 fix 模板
DEFAULT_FIX_TEMPLATES = {
    'SCAN-001': ('create_index', 'CREATE INDEX ON <table> (<column>);'),
    'SCAN-004': ('create_index', 'CREATE INDEX ON <table> (<filter_column>);'),
    'JOIN-001': ('tune_param', 'enable_nestloop=off / 强制使用 Hash Join'),
    'JOIN-002': ('tune_param', 'SET work_mem = \'256MB\'; (或更大)'),
    'MEM-001': ('tune_param', 'SET work_mem = \'256MB\';'),
    'MEM-004': ('tune_param', '调小批次,或拆分 query'),
    'SORT-003': ('rewrite_sql', '去除冗余 ORDER BY'),
    'NET-001': ('tune_param', '调整分布键或启用 redistribute'),
    'EST-001': ('update_stats', 'ANALYZE <table>;'),
    'EST-004': ('update_stats', 'ANALYZE <table>;'),
    'TYPE-001': ('rewrite_sql', '将字面量转为目标列类型'),
    'TYPE-004': ('rewrite_sql', 'LIKE 模式改为后缀通配符或全文检索'),
    'PUSH-001': ('rewrite_sql', '去掉列上的函数包装,改写谓词'),
    'PUSH-002': ('tune_param', '调整分布策略 / 表分布键'),
    'PART-001': ('rewrite_sql', '谓词改用分区键直接比较'),
    'SUBQ-001': ('rewrite_sql', '改写为 JOIN'),
    'SUBQ-006': ('rewrite_sql', '改写为 UPDATE ... FROM'),
    'REW-001': ('rewrite_sql', '大 IN 列表改写为临时表 JOIN'),
    'VEC-001': ('tune_param', '检查混合存储引擎'),
    'AGG-001': ('tune_param', 'enable_hashagg=on / enable_sort=on'),
    'AGG-002': ('tune_param', 'SET work_mem = \'256MB\';'),
    'DIST-001': ('tune_param', '调整分布列'),
    'SKEW-001': ('redesign_schema', '打散热点 / 分桶'),
    'STATS-001': ('update_stats', 'ANALYZE <table>;'),
    'GEN-001': ('rewrite_sql', '简化嵌套或拆 CTE'),
}


def extract_runtime_ms(explain_text: str) -> int | None:
    """从 'Total runtime: 0.286 ms' 提取毫秒"""
    m = re.search(r'Total runtime:\s*([\d.]+)\s*ms', explain_text)
    if m:
        return int(float(m.group(1)))
    return None


def extract_tables(explain_text: str) -> list[str]:
    """从 EXPLAIN 提取所有涉及的表名,去重保序"""
    tables = []
    seen = set()
    # 形如 "Seq Scan on <table>" / "Index Scan using <idx> on <table>"
    for m in re.finditer(r'(?:Scan|Join)\s+(?:using\s+\S+\s+)?on\s+([a-z_][a-z0-9_]*)', explain_text):
        t = m.group(1)
        if t not in seen and t != 'public':
            tables.append(t)
            seen.add(t)
    return tables


def detect_evidence(explain_text: str, target_rule: str) -> str:
    """从 EXPLAIN 自动抽取 1-2 行关键证据,辅助人工核验"""
    lines = []
    if 'Seq Scan' in explain_text:
        for line in explain_text.split('\n'):
            if 'Seq Scan' in line:
                lines.append(line.strip())
                break
    if 'Index Cond' in explain_text:
        for line in explain_text.split('\n'):
            if 'Index Cond' in line:
                lines.append(line.strip())
                break
    if 'Filter:' in explain_text:
        for line in explain_text.split('\n'):
            if 'Filter:' in line:
                lines.append(line.strip())
                break
    if 'Rows Removed by Filter' in explain_text:
        for line in explain_text.split('\n'):
            if 'Rows Removed by Filter' in line:
                lines.append(line.strip())
                break
    if 'Selected Partitions' in explain_text:
        for line in explain_text.split('\n'):
            if 'Selected Partitions' in line:
                lines.append(line.strip())
                break
    if 'SubPlan' in explain_text:
        lines.append('has SubPlan node')
    if not lines:
        lines.append('(no obvious signal)')
    return '\n'.join(lines[:3])


def detect_actually_triggered(explain_text: str, target_rule: str, warnings: list) -> dict:
    """
    基于 EXPLAIN 内容 + warnings 判断规则是否真正触发。
    返回 {actually_triggered: bool, signal: str}
    """
    signals = []

    # DELETE STATISTICS 被跳过 → 规则没真正测试
    if any('skipped unsupported' in w for w in warnings):
        return {
            'actually_triggered': False,
            'signal': 'warning: openGauss skipped the side-effect statement; rule NOT actually tested',
        }

    # 检查各种规则的物理痕迹
    if target_rule.startswith('SCAN-'):
        if 'Seq Scan' in explain_text:
            signals.append('Seq Scan present')
        # 有 LIMIT 时 cost 可能被拉低
        if explain_text.lstrip().startswith('Limit'):
            signals.append('LIMIT present (may suppress large-cost signal)')
    elif target_rule.startswith('TYPE-'):
        # implicit cast 可能在 EXPLAIN 里看不到(优化器已消除)
        if '::text' in explain_text or '::bpchar' in explain_text:
            signals.append('explicit cast visible in EXPLAIN')
        elif re.search(r'\(\w+\.\w+\)\s*=\s*\d+\b', explain_text) and 'Index Cond' in explain_text:
            signals.append('cast possibly eliminated by optimizer (look at Index Cond value type)')
        else:
            signals.append('no obvious cast signal in EXPLAIN')
    elif target_rule.startswith('TYPE-004'):
        if '~~' in explain_text and 'Filter' in explain_text:
            signals.append('LIKE ~~ with Filter (leading wildcard)')
        else:
            signals.append('LIKE pattern not in expected position')
    elif target_rule.startswith('PART-'):
        if 'Selected Partitions:' in explain_text:
            # 看 Selected Partitions 范围, 1..7 是全扫
            m = re.search(r'Selected Partitions:\s*([\d.]+)', explain_text)
            if m:
                signals.append(f'Selected Partitions: {m.group(1)}')
        if 'One-Time Filter: false' in explain_text:
            signals.append('One-Time Filter: false (PG short-circuits)')
    elif target_rule.startswith('EST-'):
        if any('skipped unsupported' in w for w in warnings):
            return {'actually_triggered': False, 'signal': 'DELETE STATISTICS skipped by openGauss'}
        # 检查 estimated rows vs actual rows 偏差
        m = re.search(r'rows=(\d+).+?actual time=[\d.]+\.\.[\d.]+ rows=(\d+)', explain_text)
        if m:
            est, act = int(m.group(1)), int(m.group(2))
            ratio = max(est, act) / max(min(est, act), 1)
            if ratio >= 10:
                signals.append(f'rows estimate/actual ratio: {ratio:.1f}x')
        if not signals:
            signals.append('EST rule needs manual review of estimate vs actual')
    elif target_rule.startswith('MEM-'):
        if 'Sort Method' in explain_text:
            for line in explain_text.split('\n'):
                if 'Sort Method' in line:
                    signals.append(line.strip())
                    break
        if 'Batches:' in explain_text and re.search(r'Batches:\s*[2-9]', explain_text):
            signals.append('Hash batches > 1 (spill signal)')
    elif target_rule.startswith('JOIN-'):
        if 'Nested Loop' in explain_text:
            signals.append('Nested Loop present')
        if 'Hash Join' in explain_text:
            signals.append('Hash Join present')
        if 'Batches:' in explain_text and re.search(r'Batches:\s*[2-9]', explain_text):
            signals.append('Hash spill detected (JOIN-002)')
        if 'loops=' in explain_text:
            loops = re.findall(r'loops=(\d+)', explain_text)
            if loops and int(max(loops)) > 100:
                signals.append(f'high loops={max(loops)} (Nested Loop iteration)')
    elif target_rule.startswith('SORT-'):
        if 'Sort' in explain_text and 'external merge' in explain_text.lower():
            signals.append('Sort Method: external merge')
        if 'Sort' in explain_text:
            sort_count = explain_text.count('Sort  (cost=')
            if sort_count >= 2:
                signals.append(f'{sort_count} Sort nodes (duplicate sort signal)')
    elif target_rule.startswith('NET-'):
        if 'Streaming' in explain_text or 'Broadcast' in explain_text or 'Redistribute' in explain_text:
            signals.append('Streaming node present')
        elif 'Gather' in explain_text or 'hash join' in explain_text.lower():
            signals.append('distributed join/streaming node present')
    elif target_rule.startswith('AGG-'):
        if 'HashAggregate' in explain_text:
            signals.append('HashAggregate present')
        if 'GroupAggregate' in explain_text:
            signals.append('GroupAggregate present')
        if 'Batches:' in explain_text and re.search(r'Batches:\s*[2-9]', explain_text):
            signals.append('HashAgg spill (Batches > 1)')
    elif target_rule.startswith('DIST-'):
        if 'Streaming' in explain_text or 'Redistribute' in explain_text or 'Broadcast' in explain_text:
            signals.append('Streaming/Redistribute/Broadcast present')
    elif target_rule.startswith('PUSH-'):
        if 'Streaming' in explain_text:
            signals.append('Streaming node present')
        if 'Remote Query' in explain_text:
            signals.append('Remote Query (no pushdown)')
        if 'Filter:' in explain_text and 'Rows Removed by Filter' in explain_text:
            for m in re.finditer(r'Filter:.*?\n.*?Rows Removed by Filter:\s*(\d+)', explain_text):
                if int(m.group(1)) > 1000:
                    signals.append(f'large Rows Removed by Filter: {m.group(1)}')
    elif target_rule.startswith('SUBQ-001'):
        if 'SubPlan' in explain_text:
            signals.append('SubPlan node present (correlated subquery NOT lifted)')
        else:
            signals.append('no SubPlan (subquery was lifted to join)')
    elif target_rule.startswith('SUBQ-006'):
        if 'UPDATE' in explain_text.upper() or 'SubPlan' in explain_text:
            signals.append('UPDATE with SubPlan present')
    elif target_rule.startswith('VEC-'):
        if 'Row Adapter' in explain_text or 'Vector Adapter' in explain_text:
            signals.append('Row/Vector Adapter present')
    elif target_rule.startswith('SKEW-'):
        if 'Hash' in explain_text and 'Batches:' in explain_text:
            signals.append('Hash with Batches (possible skew)')
        # A-time 偏差是倾斜特征
        m = re.search(r'actual time=([\d.]+)\.\.([\d.]+)', explain_text)
        if m:
            signals.append(f'actual time range present')
    elif target_rule.startswith('STATS-'):
        if any('skipped unsupported' in w for w in warnings):
            return {'actually_triggered': False, 'signal': 'openGauss does not support DELETE STATISTICS'}
    elif target_rule.startswith('GEN-'):
        # 计算计划深度
        depth = explain_text.count('\n  ->') + explain_text.count('\n    ->')
        if depth >= 5:
            signals.append(f'plan depth ~{depth}')
    elif target_rule.startswith('REW-'):
        if 'IN (' in explain_text:
            signals.append('IN list query present')

    return {
        'actually_triggered': len(signals) > 0 and 'NOT' not in ' '.join(signals),
        'signal': '; '.join(signals) if signals else '(no signal pattern matched)',
    }


def parse_annotation_from_explain(explain_text: str) -> dict:
    """
    从 EXPLAIN 文件顶部的 -- @xxx 注释提取权威元数据 + SQL。
    queries_meta.json 的 target_rule 会被截断,但 EXPLAIN 注释里保留完整 ID。
    """
    info = {'target_rule': None, 'severity': None, 'scenario': None, 'sql': None}
    lines = explain_text.split('\n')
    sql_start = None
    for i, line in enumerate(lines[:15]):
        if line.startswith('-- @target:'):
            info['target_rule'] = line.split(':', 1)[1].strip()
        elif line.startswith('-- @severity:'):
            info['severity'] = line.split(':', 1)[1].strip()
        elif line.startswith('-- @scenario:'):
            info['scenario'] = line.split(':', 1)[1].strip()
        elif line.strip() and not line.startswith('--'):
            sql_start = i
            break
    if sql_start is not None:
        sql_lines = []
        for line in lines[sql_start:]:
            stripped = line.strip()
            if not stripped or stripped.startswith('--'):
                break
            sql_lines.append(line)
        info['sql'] = '\n'.join(sql_lines).strip()
        if info['sql'].endswith(';'):
            info['sql'] = info['sql'][:-1].rstrip()
    return info


def build_case(q: dict, explain_text: str, run_meta: dict) -> dict:
    qid = q['id']
    # 优先用 EXPLAIN 里的完整 @target(如 SCAN-001),fallback 到 queries_meta.json
    annot = parse_annotation_from_explain(explain_text)
    target_rule = annot['target_rule'] or q['target_rule']
    severity = annot['severity'] or q['severity']
    scenario = annot['scenario'] or q['scenario']
    sql = annot['sql'] or q.get('sql', '')
    is_healthy = (target_rule == 'NONE')

    runtime = extract_runtime_ms(explain_text)
    tables = extract_tables(explain_text)
    evidence = detect_evidence(explain_text, target_rule)
    trigger_info = detect_actually_triggered(explain_text, target_rule, run_meta.get('warnings', []))

    # case_id
    n = int(qid[1:])
    case_id = f"OGEXP-GT-2026-{n:04d}"

    # root_causes
    if is_healthy:
        root_causes = [{
            'cause_id': 'RC-1',
            'category': 'no_real_problem',
            'severity': 'info',
            'description': 'No real performance problem; query is well-optimized.',
            'evidence': evidence,
            'verification_method': 'runtime_trace',
            'verification_detail': f'EXPLAIN ANALYZE runtime: {runtime}ms',
            'verified_by': ['build_cases.py:auto'],
        }]
        suggested_fixes = []
    else:
        category = RULE_TO_CAUSE_CATEGORY.get(target_rule, 'other')
        root_causes = [{
            'cause_id': 'RC-1',
            'category': category,
            'ogexplain_rule_id': target_rule,
            'ogexplain_rule_category': RULE_PREFIX_TO_CATEGORY.get(target_rule.split('-')[0], 'MISC-'),
            'severity': severity,
            'description': scenario,
            'evidence': evidence,
            'verification_method': 'runtime_trace' if not any('skipped' in w for w in run_meta.get('warnings', [])) else 'consensus_only',
            'verification_detail': f'EXPLAIN ANALYZE runtime: {runtime}ms; actually_triggered={trigger_info["actually_triggered"]}; signal="{trigger_info["signal"]}"',
            'verified_by': ['build_cases.py:auto'],
        }]
        fix_type, fix_sql = DEFAULT_FIX_TEMPLATES.get(target_rule, ('rewrite_sql', 'TODO'))
        suggested_fixes = [{
            'fix_id': 'FIX-1',
            'cause_id': 'RC-1',
            'type': fix_type,
            'description': f'Default fix template for {target_rule}',
            'sql_or_action': fix_sql,
            'applied': False,
            'effect_validated': False,
        }]

    return {
        'case_id': case_id,
        'version': '1.0',
        'created_at': datetime.now(timezone.utc).isoformat(),
        'updated_at': datetime.now(timezone.utc).isoformat(),
        'status': 'draft',
        'source': {
            'business_scenario': scenario,
            'database_version': 'openGauss 7.0.0-RC1',
            'database_mode': 'distributed',
            'table_size_class': 'medium',
            'data_volume_rows': 16049,  # payment table; many queries use payment/rental
            'collection_date': '2026-06-20',
            'collector_id': 'ogagila-auto',
            'anonymized': True,
            'redaction_notes': 'Synthetic Pagila schema; no real customer data',
        },
        'input': {
            'sql': sql,
            'explain_output': explain_text,
            'explain_format': 'text',
            'has_analyze': True,
            'plan_actual_runtime_ms': runtime,
            'related_tables': tables,
        },
        'ground_truth': {
            'is_problematic': not is_healthy,
            'expected_severity_if_problematic': severity if not is_healthy else None,
            'root_causes': root_causes,
            'suggested_fixes': suggested_fixes,
        },
        'annotations': [],  # 待 DBA 标注
        'validation': {
            'consensus_reached': False,
            'disputed_fields': [],
            'arbiter_id': 'pending',
            'final_decision_at': None,
            'arbiter_notes': 'Auto-generated from queries_v1.sql + ogagila EXPLAIN ANALYZE; needs DBA review.',
        },
        'tags': [target_rule, f'db-version:og7.0', f'schema:pagila', 'auto-generated'] +
                (['side-effect', 'skipped-statements'] if run_meta.get('has_side_effect') and not is_healthy else []),
        # 额外字段,标注本次自动评估结果
        '_auto_eval': {
            'target_rule_designed': target_rule,
            'actually_triggered': trigger_info['actually_triggered'],
            'trigger_signal': trigger_info['signal'],
            'run_warnings': run_meta.get('warnings', []),
        },
    }


def main():
    global META_PATH, EXPLAINS_DIR, OUTPUT_DIR

    p = argparse.ArgumentParser(description='Build ground-truth case JSON from ogagila EXPLAIN outputs')
    p.add_argument('--meta', default=str(DEFAULT_META),
                   help=f'Path to queries_meta.json (default: {DEFAULT_META})')
    p.add_argument('--explains-dir', default=str(DEFAULT_EXPLAINS),
                   help=f'Directory with Q*.explain + Q*.meta.json files (default: {DEFAULT_EXPLAINS})')
    p.add_argument('--output-dir', default=str(DEFAULT_OUTPUT),
                   help=f'Output directory for case JSON files (default: {DEFAULT_OUTPUT})')
    args = p.parse_args()

    META_PATH = Path(args.meta)
    EXPLAINS_DIR = Path(args.explains_dir)
    OUTPUT_DIR = Path(args.output_dir)
    OUTPUT_DIR.mkdir(parents=True, exist_ok=True)

    if not META_PATH.exists():
        sys.exit(f"missing {META_PATH}")
    meta = json.load(open(META_PATH, encoding='utf-8'))
    queries = meta['queries']

    # index.json(可选,加速检测)
    index_data = None
    index_path = EXPLAINS_DIR / 'index.json'
    if index_path.exists():
        try:
            index_data = json.load(open(index_path))
        except:
            pass

    print(f"[build_cases] {len(queries)} queries from queries_meta.json")

    case_index = []
    trigger_summary = {}
    skipped = []
    n_ok = n_failed = 0

    for q in queries:
        qid = q['id']
        explain_path = EXPLAINS_DIR / f"{qid}.explain"
        meta_run_path = EXPLAINS_DIR / f"{qid}.meta.json"

        if not explain_path.exists() or not meta_run_path.exists():
            print(f"  [SKIP] {qid}: missing files")
            skipped.append(qid)
            continue

        explain_text = explain_path.read_text(encoding='utf-8')
        run_meta = json.loads(meta_run_path.read_text(encoding='utf-8'))

        case = build_case(q, explain_text, run_meta)
        case_path = OUTPUT_DIR / f"{case['case_id']}.json"
        case_path.write_text(json.dumps(case, indent=2, ensure_ascii=False), encoding='utf-8')

        case_index.append({
            'case_id': case['case_id'],
            'source_query_id': qid,
            'target_rule': case['_auto_eval']['target_rule_designed'],
            'is_healthy': case['ground_truth']['is_problematic'] == False,
            'actually_triggered': case['_auto_eval']['actually_triggered'],
            'explain_file': str(explain_path),
            'case_file': str(case_path),
        })

        # 统计
        rule = case['_auto_eval']['target_rule_designed']
        trigger_summary.setdefault(rule, {'total': 0, 'triggered': 0, 'skipped': 0})
        trigger_summary[rule]['total'] += 1
        if not case['_auto_eval']['actually_triggered']:
            trigger_summary[rule]['skipped'] += 1
        else:
            trigger_summary[rule]['triggered'] += 1

        n_ok += 1

    # 写 case_index.json
    (OUTPUT_DIR.parent / 'case_index.json').write_text(
        json.dumps({
            'version': '1.0',
            'generated_at': datetime.now(timezone.utc).isoformat(),
            'total_cases': len(case_index),
            'skipped_due_to_missing_files': skipped,
            'cases': case_index,
        }, indent=2, ensure_ascii=False),
        encoding='utf-8'
    )

    # 写 trigger_coverage.md
    lines = [
        "# Trigger Coverage Report",
        "",
        f"**Generated:** {datetime.now(timezone.utc).isoformat()}",
        f"**Source:** ogagila repo + queries_meta.json",
        f"**Total cases:** {len(case_index)}",
        "",
        "## Per-rule trigger coverage",
        "",
        "| Rule | Designed | Actually Triggered | Skipped (not really triggered) | Trigger Rate |",
        "|------|----------|--------------------|--------------------------------|--------------|",
    ]
    for rule in sorted(trigger_summary.keys()):
        s = trigger_summary[rule]
        rate = f"{100 * s['triggered'] / s['total']:.0f}%" if s['total'] else "—"
        lines.append(f"| {rule} | {s['total']} | {s['triggered']} | {s['skipped']} | {rate} |")

    lines += [
        "",
        "## Notes",
        "",
        "- **Skipped** = EXPLAIN shows that the rule's intended condition did NOT actually manifest (e.g. optimizer eliminated implicit cast, LIMIT suppressed cost signal, openGauss skipped side-effect statements like DELETE STATISTICS).",
        "- These cases still have ground truth `is_problematic: true` per the original design intent — but the runtime evidence is weaker. Mark as needing manual review.",
        "- For healthy cases (target_rule = NONE), no trigger expected — they are 'true negatives'.",
        "",
    ]

    (OUTPUT_DIR.parent / 'trigger_coverage.md').write_text('\n'.join(lines), encoding='utf-8')

    print(f"\n[done] {n_ok} cases generated → {OUTPUT_DIR}")
    print(f"[index] {OUTPUT_DIR.parent}/case_index.json")
    print(f"[report] {OUTPUT_DIR.parent}/trigger_coverage.md")

    # 触发率总览
    triggered = sum(1 for c in case_index if c['actually_triggered'])
    not_triggered = sum(1 for c in case_index if not c['actually_triggered'])
    print(f"\n[stats] triggered={triggered}, not_triggered={not_triggered}")
    print(f"[stats] trigger_rate = {triggered/(triggered+not_triggered):.1%}")


if __name__ == '__main__':
    main()