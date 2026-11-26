#!/usr/bin/env bash
# CPU flamegraphs (D-149, M9.2c; `just profile`), sampled in process by pprof: no perf, no root.
#
#   scripts/profile.sh bench [filter]   the criterion benches (crates/*/benches: ingest, render,
#                                       logs), each sampled for UNIWRT_PROFILE_S seconds (10)
#                                       without measuring; a graph per bench in
#                                       target/criterion/<group>/<bench>/profile/flamegraph.svg
#   scripts/profile.sh serve [bin]      the controller serving the 50-node / 1,000-client demo,
#                                       sampled from its start until it stops: it settles
#                                       (UNIWRT_PERF_SETTLE_S, 60 s), then takes scripts/perf.sh's
#                                       API reads for UNIWRT_PROFILE_S seconds (30); the graph in
#                                       target/profile/serve.svg. Without [bin] it builds one: the
#                                       `profiling` cargo profile (release, symbols kept) with the
#                                       `profiling` feature (`serve --profile-cpu`).
#
# The filter is criterion's (a regex over "<group>/<bench>", e.g. `ingest_50x1000/topology`).
set -euo pipefail
cd "$(dirname "$0")/.."

die() { echo "profile: $*" >&2; exit 2; }

BENCHES=(-p uniwrt-engine --bench ingest -p uniwrt-render --bench render -p uniwrt-syslog --bench logs)

bench() {
  cargo bench --locked "${BENCHES[@]}" -- --profile-time "${UNIWRT_PROFILE_S:-10}" "$@"
}

serve_profile() {
  local bin=${1:-} out=target/profile/serve.svg deadline times
  if [[ -z $bin ]]; then
    cargo build --locked --profile profiling --features profiling -p uniwrt
    bin=target/profiling/uniwrt
  fi
  # shellcheck source=scripts/perf.sh
  source scripts/perf.sh
  mkdir -p "$(dirname "$out")"
  rm -f "$out"
  limit=120 serve "$bin" --demo --demo-access-points 50 --demo-clients 1000 \
    --profile-cpu "$PWD/$out"
  echo "profile: the demo settles for ${UNIWRT_PERF_SETTLE_S:-60} s" >&2
  sleep "${UNIWRT_PERF_SETTLE_S:-60}"
  kill -0 "$pid" 2>/dev/null || die "the demo exited while it settled"
  times=$(temp)
  deadline=$((SECONDS + ${UNIWRT_PROFILE_S:-30}))
  echo "profile: the API's reads for ${UNIWRT_PROFILE_S:-30} s" >&2
  api_login
  while ((SECONDS < deadline)); do api_reads "$times"; done
  kill -INT "$pid"
  wait "$pid" || true
  [[ -s $out ]] || die "serve wrote no flamegraph (built without the profiling feature?)"
  echo "$out"
}

case ${1:-} in
  bench) shift; bench "$@" ;;
  serve) [[ $# -le 2 ]] || die "usage: $0 serve [bin]"; serve_profile "${2:-}" ;;
  *) die "usage: $0 bench [filter] | serve [bin]" ;;
esac
