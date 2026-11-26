#!/usr/bin/env bash
# The performance budgets (D-149). Each check measures one thing, compares it with its budget in
# scripts/perf/budgets.json (or $UNIWRT_PERF_BUDGETS), prints one line, and exits 1 when the value
# is over the budget. In GitHub Actions the line also goes to the job summary.
#
#   scripts/perf.sh js <dist>           initial JS: index.html's scripts and module preloads, gzip -9
#   scripts/perf.sh binary <bin>        the binary's size on disk
#   scripts/perf.sh cold-start <bin>    `serve` on an empty data directory until `healthcheck` passes
#   scripts/perf.sh idle <bin>          `serve --demo` with 10 access points and 200 clients, left
#                                       alone: its peak RSS (heap and code shown apart) and its CPU
#                                       over a minute
#   scripts/perf.sh api <bin>           `serve --demo` with 50 access points and 1,000 clients: the
#                                       p99 of the API's reads, one at a time over one connection
#   scripts/perf.sh agent-rss <file>    the agent's RSS in bytes, as `just test-agent` wrote it
#                                       (crates/engine/tests/agent.rs); the file's name labels it
#
# UNIWRT_PERF_SETTLE_S (default 60) is how long a demo runs before it is measured, and
# UNIWRT_PERF_WINDOW_S (default 60) how long `idle` measures. scripts/profile.sh sources this file
# for its demo and the API's reads.
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
budgets=${UNIWRT_PERF_BUDGETS:-$here/perf/budgets.json}

die() { echo "perf: $*" >&2; exit 2; }

# What a check started and made, stopped and removed at exit.
pids=() temps=()
cleanup() {
  local p
  for p in "${pids[@]}"; do kill "$p" 2>/dev/null && wait "$p" 2>/dev/null; done
  rm -rf "${temps[@]}"
}
trap cleanup EXIT
temp() { local t; t=$(mktemp "$@"); temps+=("$t"); echo "$t"; }

# The budget's maximum and unit, as "max unit".
budget() { jq -er --arg k "$1" '.[$k] | "\(.max) \(.unit)"' "$budgets" || die "no budget '$1' in $budgets"; }

# A value in its unit, for people: bytes as kB or MB (decimal), a percentage with two decimals,
# anything else as is.
human() {
  case $2 in
    bytes) awk -v v="$1" 'BEGIN { if (v >= 1e6) printf "%.2f MB", v / 1e6; else printf "%.1f kB", v / 1e3 }' ;;
    percent) awk -v v="$1" 'BEGIN { printf "%.2f %%", v }' ;;
    *) echo "$1 $2" ;;
  esac
}

# report <name> <budget key> <value>: one line, and the verdict as the exit status. Values may
# have decimals.
report() {
  local max unit line status=0
  read -r max unit <<<"$(budget "$2")"
  line="$1: $(human "$3" "$unit") (budget $(human "$max" "$unit"))"
  if awk -v v="$3" -v m="$max" 'BEGIN { exit !(v > m) }'; then line="$line, OVER BUDGET"; status=1; else line="$line, ok"; fi
  echo "$line"
  if [[ -n ${GITHUB_STEP_SUMMARY:-} ]]; then echo "- $line" >>"$GITHUB_STEP_SUMMARY"; fi
  return "$status"
}

js() {
  local dist=$1 total=0 path n
  [[ -f $dist/index.html ]] || die "$dist/index.html not found"
  # Scripts (src) and module preloads (href) the page loads before any route's own chunk.
  while read -r path; do
    [[ -f $dist$path ]] || die "$dist$path (named in index.html) not found"
    n=$(gzip -9c "$dist$path" | wc -c)
    total=$((total + n))
  done < <(grep -oE '(src|href)="/[^"]+\.js"' "$dist/index.html" | sed -E 's/^[a-z]+="//; s/"$//' | sort -u)
  ((total > 0)) || die "no scripts in $dist/index.html"
  report "Initial JS (gzip)" initial_js_gzip "$total"
}

binary() {
  [[ -f $1 ]] || die "$1 not found"
  report "Binary ($(basename "$1"))" binary "$(stat -c %s "$1")"
}

# serve <bin> [flag…]: starts `serve` on a fresh data directory with the flags, sets `pid`,
# `http` and `https`, and returns once `healthcheck` passes (dies after `limit` seconds, 30 by
# default). Stopped and removed at exit.
serve() {
  local bin=$1 log data started
  shift
  [[ -x $bin ]] || die "$bin not found or not executable"
  http=127.0.0.1:${UNIWRT_PERF_HTTP_PORT:-18280} https=127.0.0.1:${UNIWRT_PERF_HTTPS_PORT:-18643}
  data=$(temp -d) log=$(temp)
  started=$(date +%s)
  "$bin" serve --http-listen "$http" --https-listen "$https" --data-dir "$data" --syslog-listen off \
    "$@" >"$log" 2>&1 &
  pid=$!
  pids+=("$pid")
  until "$bin" healthcheck --http-listen "$http" >/dev/null 2>&1; do
    kill -0 "$pid" 2>/dev/null || { cat "$log" >&2; die "serve exited before it answered"; }
    (($(date +%s) - started < ${limit:-30})) || die "serve didn't answer within ${limit:-30} s"
    sleep 0.01
  done
}

cold_start() {
  local start end
  start=$(date +%s%N)
  serve "$1"
  end=$(date +%s%N)
  report "Cold start" cold_start "$(((end - start) / 1000000))"
}

# demo <bin> <access points> <clients>: a sized demo, started and left to settle.
demo() {
  limit=120 serve "$1" --demo --demo-access-points "$2" --demo-clients "$3"
  sleep "${UNIWRT_PERF_SETTLE_S:-60}"
  kill -0 "$pid" 2>/dev/null || die "the demo exited while it settled"
}

# The process's CPU time so far, in clock ticks (utime + stime; after the command's parentheses).
cpu_ticks() { sed -E 's/^.*\) //' "/proc/$1/stat" | awk '{ print $12 + $13 }'; }
# VmRSS, then its heap (RssAnon) and code (RssFile: the binary's pages) parts, in bytes.
rss_bytes() { awk '/^VmRSS:/ { r = $2 } /^RssAnon:/ { a = $2 } /^RssFile:/ { f = $2 } END { print r * 1024, a * 1024, f * 1024 }' "/proc/$1/status"; }

idle() {
  local window=${UNIWRT_PERF_WINDOW_S:-60} before after peak=0 heap=0 code=0 rss anon file hz status=0
  demo "$1" 10 200
  hz=$(getconf CLK_TCK)
  before=$(cpu_ticks "$pid")
  for ((s = 0; s < window; s++)); do
    read -r rss anon file <<<"$(rss_bytes "$pid")"
    if ((rss > peak)); then peak=$rss heap=$anon code=$file; fi
    sleep 1
  done
  after=$(cpu_ticks "$pid")
  report "Idle RSS (10 nodes, 200 clients; heap $(human "$heap" bytes), code $(human "$code" bytes))" \
    idle_rss "$peak" || status=1
  report "Idle CPU (10 nodes, 200 clients)" idle_cpu \
    "$(awk -v t=$((after - before)) -v hz="$hz" -v w="$window" 'BEGIN { printf "%.3f", 100 * t / hz / w }')" ||
    status=1
  return "$status"
}

# The reads the UI makes most, each answered in full (paths below /api/v1; node 1 is the gateway).
API_READS=(auth/me nodes nodes/1 clients topology events alerts site drift revisions discovered
  jobs rf logs "metrics/series?node=1" services/dns/stats floors)
API_ROUNDS=${UNIWRT_PERF_API_ROUNDS:-20}

# api_login: signs in to the demo, keeping the session's cookie in $jar.
api_login() {
  jar=$(temp)
  curl -fsSk -c "$jar" -H 'x-uniwrt-csrf: 1' -H 'content-type: application/json' \
    -d '{"username":"demo","password":"demo"}' "https://$https/api/v1/auth/login" >/dev/null ||
    die "cannot sign in to the demo"
}

# api_reads <file>: reads API_READS API_ROUNDS times over one connection (signed in by
# api_login), each read's status and time in seconds as a line of <file>.
api_reads() {
  local config
  config=$(temp)
  # One curl, one connection: each request's time without the TLS handshake.
  for ((r = 0; r < API_ROUNDS; r++)); do
    for path in "${API_READS[@]}"; do
      printf 'url = "https://%s/api/v1/%s"\noutput = "/dev/null"\n' "$https" "$path"
    done
  done >"$config"
  curl -sSk -b "$jar" -K "$config" -w '%{http_code} %{time_total}\n' >"$1" ||
    die "the API reads failed"
}

api() {
  local times p99
  demo "$1" 50 1000
  times=$(temp)
  api_login
  api_reads "$times"
  if awk '$1 !~ /^2/ { bad = 1 } END { exit !bad }' "$times"; then
    die "some reads failed: $(awk '$1 !~ /^2/' "$times" | sort | uniq -c | head -3)"
  fi
  # The nearest-rank p99 in ms.
  p99=$(awk '{ print $2 * 1000 }' "$times" | sort -n |
    awk '{ v[NR] = $1 } END { i = int(NR * 0.99 + 0.999999); printf "%.1f", v[i] }')
  report "API p99 (50 nodes, 1,000 clients; $(wc -l <"$times") reads)" api_p99 "$p99"
}

agent_rss() {
  local value
  value=$(tr -d '[:space:]' <"$1") || die "$1 not readable"
  [[ $value =~ ^[0-9]+$ ]] || die "$1 holds no byte count"
  report "Agent RSS ($(basename "$1"))" agent_rss "$value"
}

# Sourced (scripts/profile.sh), it only defines the functions above.
[[ ${BASH_SOURCE[0]} == "$0" ]] || return 0

case ${1:-} in
  js) [[ $# -eq 2 ]] || die "usage: $0 js <dist>"; js "$2" ;;
  binary) [[ $# -eq 2 ]] || die "usage: $0 binary <bin>"; binary "$2" ;;
  cold-start) [[ $# -eq 2 ]] || die "usage: $0 cold-start <bin>"; cold_start "$2" ;;
  idle) [[ $# -eq 2 ]] || die "usage: $0 idle <bin>"; idle "$2" ;;
  api) [[ $# -eq 2 ]] || die "usage: $0 api <bin>"; api "$2" ;;
  agent-rss) [[ $# -eq 2 ]] || die "usage: $0 agent-rss <file>"; agent_rss "$2" ;;
  *) die "usage: $0 js <dist> | binary <bin> | cold-start <bin> | idle <bin> | api <bin> | agent-rss <file>" ;;
esac
