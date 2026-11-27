#!/usr/bin/env bash
# The fuzz targets (fuzz/; M9.3b, D-150): cargo-fuzz (libFuzzer, AddressSanitizer, debug
# assertions and overflow checks) on a pinned nightly, which only this script uses.
#
#   scripts/fuzz.sh run [SECONDS] [TARGET…]  seeds the corpora, builds, then fuzzes each target
#                                             (all by default) for SECONDS (60) and stops at the
#                                             first finding: its input is in fuzz/artifacts/<target>/
#   scripts/fuzz.sh repro TARGET FILE         runs one input again, with the panic's message
#   scripts/fuzz.sh cmin [TARGET…]            minimizes the corpora (the weekly run)
#   scripts/fuzz.sh build | seed | list | toolchain
# The corpora are fuzz/corpus/<target>/ (gitignored; CI caches them), seeded from
# fuzz/seeds/<target>/ and the crates' fixtures. Each run's libFuzzer log is fuzz/logs/<target>.log.
# Cargo.lock is the workspace's, copied in first, so the targets fuzz the versions that ship.
set -euo pipefail
cd "$(dirname "$0")/.."

toolchain=${UNIWRT_FUZZ_TOOLCHAIN:-nightly-2026-11-25}

# The host's target, named: a cargo-fuzz built for musl (as CI installs it) defaults to musl.
fuzz() {
  local command=$1 host
  shift
  host=$(rustc "+$toolchain" -vV | sed -n 's/^host: //p')
  cargo "+$toolchain" fuzz "$command" --fuzz-dir fuzz --target "$host" "$@"
}

list() { sed -n 's/^name = "\(.*\)"$/\1/p' fuzz/Cargo.toml | grep -v '^uniwrt-fuzz$'; }

build() {
  cp Cargo.lock fuzz/Cargo.lock
  fuzz build
}

# copy TARGET FILE…: adds the files to TARGET's corpus, named by their paths.
copy() {
  local dir=fuzz/corpus/$1 file
  shift
  for file in "$@"; do
    cp "$file" "$dir/seed-${file//\//_}"
  done
}

seed() {
  local target file release name n
  for target in $(list); do
    mkdir -p "fuzz/corpus/$target"
    if [[ -d fuzz/seeds/$target ]]; then copy "$target" fuzz/seeds/"$target"/*; fi
  done
  copy uci crates/uci/fixtures/cases/* crates/uci/fixtures/openwrt/*/*
  copy facts crates/node/fixtures/collect/*.json
  copy agent_stream crates/node/fixtures/events/*.jsonl
  copy feeds crates/feeds/fixtures/*
  copy firmware crates/firmware/src/sigstore/testdata/* crates/firmware/data/sigstore-trusted-root.json
  gzip -dc crates/feeds/fixtures/*.Packages.gz >fuzz/corpus/feeds/seed-Packages
  for release in crates/uci/fixtures/openwrt/*/; do
    name=$(basename "$release")
    # `uci export` of a whole node: each package's .out file is its export.
    cat "$release"*.out >"fuzz/corpus/import/seed-export-$name"
    # A `sysupgrade -b` archive of the node's /etc/config.
    file=$(mktemp -d)
    mkdir -p "$file/etc/config"
    find "$release" -maxdepth 1 -type f ! -name '*.out' -exec cp {} "$file/etc/config/" \;
    tar -czf "fuzz/corpus/archive/seed-backup-$name.tar.gz" -C "$file" etc
    rm -rf "$file"
  done
  for file in crates/node/fixtures/collect/*.json; do
    name=$(basename "$file" .json)
    # A collect answer as the agent sends it, and each set's sources alone.
    jq -c '{id: 1, ok: .}' "$file" >"fuzz/corpus/agent_stream/seed-answer-$name"
    n=0
    while IFS= read -r set; do
      n=$((n + 1))
      printf '%s' "$set" >"fuzz/corpus/facts/seed-$name-set$n"
    done < <(jq -c '(.sets // {})[]' "$file")
  done
}

run() {
  local seconds=${1:-60} targets target log
  shift || true
  targets=("$@")
  if ((${#targets[@]} == 0)); then mapfile -t targets < <(list); fi
  build
  seed
  mkdir -p fuzz/logs
  for target in "${targets[@]}"; do
    log=fuzz/logs/$target.log
    if fuzz run "$target" -- -max_total_time="$seconds" -timeout=10 -rss_limit_mb=2048 \
      -print_final_stats=1 >"$log" 2>&1; then
      printf '✓ %s: %s runs, %s inputs in the corpus\n' "$target" \
        "$(sed -n 's/^stat::number_of_executed_units: *//p' "$log" | tail -n 1)" \
        "$(find "fuzz/corpus/$target" -type f | wc -l)"
    else
      printf '✗ %s: a finding (fuzz/artifacts/%s/); the end of %s:\n' "$target" "$target" "$log"
      tail -n 60 "$log"
      return 1
    fi
  done
}

case ${1:-} in
run) run "${@:2}" ;;
repro) build && fuzz run "$2" "$3" ;;
cmin)
  build
  targets=("${@:2}")
  if ((${#targets[@]} == 0)); then mapfile -t targets < <(list); fi
  for target in "${targets[@]}"; do fuzz cmin "$target" -- -rss_limit_mb=2048; done
  ;;
build) build ;;
seed) seed ;;
list) list ;;
toolchain) echo "$toolchain" ;;
*)
  sed -n '2,15p' "$0"
  exit 2
  ;;
esac
