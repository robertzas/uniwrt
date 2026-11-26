#!/usr/bin/env bash
# Tests for scripts/perf.sh (D-149): each check passes under its budget and fails over it.
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
fail() { echo "FAIL: $*" >&2; exit 1; }

budgets() {
  printf '{"initial_js_gzip":{"max":%s,"unit":"bytes"},"binary":{"max":%s,"unit":"bytes"},"cold_start":{"max":1000,"unit":"ms"}}' "$1" "$2" >"$tmp/budgets.json"
}
perf() { UNIWRT_PERF_BUDGETS=$tmp/budgets.json GITHUB_STEP_SUMMARY='' "$here/perf.sh" "$@"; }

# A page with an entry script, a preload, a stylesheet (not counted) and a lazy chunk (not named).
mkdir -p "$tmp/dist/assets"
head -c 20000 /dev/urandom >"$tmp/dist/assets/index-a.js" # random bytes barely compress
head -c 10000 /dev/urandom >"$tmp/dist/assets/vendor-b.js"
head -c 50000 /dev/urandom >"$tmp/dist/assets/lazy-c.js"
head -c 50000 /dev/urandom >"$tmp/dist/assets/index-d.css"
cat >"$tmp/dist/index.html" <<'EOF'
<script type="module" crossorigin src="/assets/index-a.js"></script>
<link rel="modulepreload" crossorigin href="/assets/vendor-b.js">
<link rel="stylesheet" crossorigin href="/assets/index-d.css">
EOF

budgets 40000 1000
out=$(perf js "$tmp/dist") || fail "js under budget failed: $out"
[[ $out == "Initial JS (gzip): 30."*" kB (budget 40.0 kB), ok" ]] || fail "js line: $out"

budgets 25000 1000
if out=$(perf js "$tmp/dist"); then fail "js over budget passed: $out"; fi
[[ $out == *"OVER BUDGET" ]] || fail "js over-budget line: $out"

echo '<script src="/assets/missing.js"></script>' >"$tmp/dist/index.html"
set +e; perf js "$tmp/dist" >/dev/null 2>&1; status=$?; set -e
((status == 2)) || fail "a missing script should exit 2, got $status"

truncate -s 1500000 "$tmp/bin"
budgets 1 2000000
out=$(perf binary "$tmp/bin") || fail "binary under budget failed: $out"
[[ $out == "Binary (bin): 1.50 MB (budget 2.00 MB), ok" ]] || fail "binary line: $out"
budgets 1 1000000
if perf binary "$tmp/bin" >/dev/null; then fail "binary over budget passed"; fi

# The summary gets the same line.
UNIWRT_PERF_BUDGETS=$tmp/budgets.json GITHUB_STEP_SUMMARY=$tmp/summary "$here/perf.sh" binary "$tmp/bin" >/dev/null || true
grep -qx -- '- Binary (bin): 1.50 MB (budget 1.00 MB), OVER BUDGET' "$tmp/summary" || fail "summary: $(cat "$tmp/summary")"

# The agent's RSS comes from a file named for its release.
printf '{"agent_rss":{"max":%s,"unit":"bytes"}}' 4000000 >"$tmp/budgets.json"
echo 2100000 >"$tmp/25.12.6"
out=$(perf agent-rss "$tmp/25.12.6") || fail "agent-rss under budget failed: $out"
[[ $out == "Agent RSS (25.12.6): 2.10 MB (budget 4.00 MB), ok" ]] || fail "agent-rss line: $out"
echo 'peak 23 MB' >"$tmp/24.10.8"
set +e; perf agent-rss "$tmp/24.10.8" >/dev/null 2>&1; status=$?; set -e
((status == 2)) || fail "a file without a byte count should exit 2, got $status"

# Budgets and values may have decimals (a CPU share in percent).
printf '{"agent_rss":{"max":0.5,"unit":"percent"}}' >"$tmp/budgets.json"
echo 1 >"$tmp/cpu"
if out=$(perf agent-rss "$tmp/cpu"); then fail "a value over a decimal budget passed: $out"; fi
[[ $out == "Agent RSS (cpu): 1.00 % (budget 0.50 %), OVER BUDGET" ]] || fail "percent line: $out"

echo "perf.sh: all tests passed"
