#!/usr/bin/env bash
# The performance budgets (D-149). Each check measures one thing, compares it with its budget in
# scripts/perf/budgets.json (or $UNIWRT_PERF_BUDGETS), prints one line, and exits 1 when the value
# is over the budget. In GitHub Actions the line also goes to the job summary.
#
#   scripts/perf.sh js <dist>           initial JS: index.html's scripts and module preloads, gzip -9
#   scripts/perf.sh binary <bin>        the binary's size on disk
#   scripts/perf.sh cold-start <bin>    `serve` on an empty data directory until `healthcheck` passes
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
budgets=${UNIWRT_PERF_BUDGETS:-$here/perf/budgets.json}

die() { echo "perf: $*" >&2; exit 2; }

# The budget's maximum and unit, as "max unit".
budget() { jq -er --arg k "$1" '.[$k] | "\(.max) \(.unit)"' "$budgets" || die "no budget '$1' in $budgets"; }

# A value in its unit, for people: bytes as kB or MB (decimal), ms as is.
human() {
  case $2 in
    bytes) awk -v v="$1" 'BEGIN { if (v >= 1e6) printf "%.2f MB", v / 1e6; else printf "%.1f kB", v / 1e3 }' ;;
    *) echo "$1 $2" ;;
  esac
}

# report <name> <budget key> <value>: one line, and the verdict as the exit status.
report() {
  local max unit line status=0
  read -r max unit <<<"$(budget "$2")"
  line="$1: $(human "$3" "$unit") (budget $(human "$max" "$unit"))"
  if (($3 > max)); then line="$line, OVER BUDGET"; status=1; else line="$line, ok"; fi
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

cold_start() {
  local bin=$1 data http=127.0.0.1:${UNIWRT_PERF_HTTP_PORT:-18280} https=127.0.0.1:${UNIWRT_PERF_HTTPS_PORT:-18643}
  local start end pid
  [[ -x $bin ]] || die "$bin not found or not executable"
  data=$(mktemp -d)
  start=$(date +%s%N)
  "$bin" serve --http-listen "$http" --https-listen "$https" --data-dir "$data" --syslog-listen off \
    >"$data.log" 2>&1 &
  pid=$!
  # shellcheck disable=SC2064 # expand now: the trap must kill this run's server and directory
  trap "kill $pid 2>/dev/null; rm -rf '$data' '$data.log'" EXIT
  until "$bin" healthcheck --http-listen "$http" >/dev/null 2>&1; do
    kill -0 "$pid" 2>/dev/null || { cat "$data.log" >&2; die "serve exited before it answered"; }
    (($(date +%s%N) - start < 30000000000)) || die "serve didn't answer within 30 s"
    sleep 0.01
  done
  end=$(date +%s%N)
  report "Cold start" cold_start "$(((end - start) / 1000000))"
}

case ${1:-} in
  js) [[ $# -eq 2 ]] || die "usage: $0 js <dist>"; js "$2" ;;
  binary) [[ $# -eq 2 ]] || die "usage: $0 binary <bin>"; binary "$2" ;;
  cold-start) [[ $# -eq 2 ]] || die "usage: $0 cold-start <bin>"; cold_start "$2" ;;
  *) die "usage: $0 js <dist> | binary <bin> | cold-start <bin>" ;;
esac
