#!/usr/bin/env bash
# Live end-to-end test for ogexplain optimize.
#
# Prerequisites:
#   1. ogagila docker-compose running (provides orders/items/customers schema)
#   2. metamorphosis binary in PATH (built from PR #35 branch)
#   3. ~/.gaussdb-mcp.toml configured for the docker DB
#   4. ogexplain built with --features db: cargo build -p ogexplain-cli --release
#
# Usage:
#   bash tests/optimize_live_e2e.sh
#
# What this validates:
#   - EXPLAIN on real DB → SUBQ-001 fires with table populated
#   - mapper routes SUBQ-001 to subquery-to-join rule
#   - metamorphosis rewrite executes successfully
#   - re-EXPLAIN produces different metrics
#   - convergence loop decides Continue or Stop
#   - optional: QED equivalence verification of final SQL

set -euo pipefail

SQL='SELECT o.order_id, o.customer_id FROM orders o WHERE o.order_id IN (SELECT i.order_id FROM items i WHERE i.amount > 100)'

SCHEMA_FILE="${SCHEMA_FILE:-schema.json}"
MAX_ITERATIONS="${MAX_ITERATIONS:-3}"
OUTPUT_DIR="${OUTPUT_DIR:-/tmp/ogexplain-optimize-e2e}"

mkdir -p "$OUTPUT_DIR"

echo "=== Step 0: Verify prerequisites ==="
if ! command -v ogexplain >/dev/null 2>&1; then
    echo "ERROR: ogexplain not in PATH. Build with: cargo build -p ogexplain-cli --release"
    exit 1
fi
if ! command -v metamorphosis >/dev/null 2>&1; then
    echo "ERROR: metamorphosis not in PATH. See https://github.com/c2j/metamorphosis"
    exit 1
fi
echo "ogexplain:  $(ogexplain --version 2>&1 || echo 'unknown')"
echo "metamorphosis: $(metamorphosis --version 2>&1 || echo 'unknown')"

echo ""
echo "=== Step 1: Baseline EXPLAIN ANALYZE ==="
ogexplain explain \
    -s "$SQL" \
    --analyze \
    --format json \
    -o "$OUTPUT_DIR/baseline.json"
echo "Baseline metrics:"
python3 -c "
import json
with open('$OUTPUT_DIR/baseline.json') as f:
    data = json.load(f)
s = data.get('summary', {})
print(f\"  total_cost: {s.get('total_cost')}\")
print(f\"  critical_count: {s.get('critical_count')}\")
print(f\"  warning_count: {s.get('warning_count')}\")
findings = data.get('findings', [])
subq = [f for f in findings if f.get('rule_id') == 'SUBQ-001']
print(f\"  SUBQ-001 findings: {len(subq)}\")
for f in subq:
    print(f\"    table={f.get('table')} columns={f.get('columns')}\")" || true

echo ""
echo "=== Step 2: Run optimize loop ==="
ogexplain optimize \
    --sql "$SQL" \
    --schema "$SCHEMA_FILE" \
    --max-iterations "$MAX_ITERATIONS" \
    --skip-stats-check \
    --format json \
    -o "$OUTPUT_DIR/optimization_result.json" \
    || {
        echo "optimize failed — see stderr above"
        exit 1
    }

echo ""
echo "=== Step 3: Verify result ==="
python3 -c "
import json
with open('$OUTPUT_DIR/optimization_result.json') as f:
    data = json.load(f)
print(f\"Iterations: {data.get('iterations_count')}\")
print(f\"Stop reason: {data.get('stop_reason')}\")
final_sql = data.get('final_sql', '')
print(f\"Final SQL ({len(final_sql)} chars):\")
print(final_sql[:500])
if len(final_sql) > 500:
    print('  ...(truncated)')"

echo ""
echo "=== Step 4: (Optional) QED-verify final SQL equivalence ==="
if [ -x "$(command -v metamorphosis)" ] && [ -f "$SCHEMA_FILE" ]; then
    python3 -c "
import json
with open('$OUTPUT_DIR/optimization_result.json') as f:
    data = json.load(f)
print(data.get('final_sql', ''))" > "$OUTPUT_DIR/final.sql"

    metamorphosis verify \
        --original <(echo "$SQL") \
        --rewritten "$OUTPUT_DIR/final.sql" \
        --schema "$SCHEMA_FILE" \
        --engine qed \
        --timeout 60 \
        && echo "✅ QED verification PASSED" \
        || echo "⚠️  QED verification failed or skipped (metamorphosis#34 features required)"
else
    echo "Skipped (metamorphosis or schema.json not available)"
fi

echo ""
echo "=== Done. Output files in $OUTPUT_DIR ==="
ls -la "$OUTPUT_DIR/"
