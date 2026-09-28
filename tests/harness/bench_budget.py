"""Fail if `obj2cad bench --json` exceeds the budget in docs/PERFORMANCE.md (M4).

Usage: obj2cad bench --synthetic 1000 --json | python tests/harness/bench_budget.py 1500
"""

import json
import sys

budget_ms = float(sys.argv[1])
rows = json.load(sys.stdin)
worst = max(rows, key=lambda r: r["total_ms"])
for r in rows:
    print(f"{r['input']}: {r['total_ms']:.0f} ms total "
          f"(parse {r['parse_ms']:.0f}, convert {r['convert_ms']:.0f}, hash {r['hash_ms']:.0f}, "
          f"write {r['write_ms']:.0f}), {r['mb_per_s']:.0f} MB/s")
if worst["total_ms"] > budget_ms:
    print(f"FAIL: {worst['total_ms']:.0f} ms exceeds the {budget_ms:.0f} ms budget")
    sys.exit(1)
print(f"OK: within the {budget_ms:.0f} ms budget")
