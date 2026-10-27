#!/usr/bin/env bash
# Lists the crates that Cargo.lock adds or changes against a git revision (default: HEAD), with
# their crates.io publish dates, and fails if any is younger than the 7-day cooldown (D-029).
# Pin a fresh one back with `cargo update -p <crate> --precise <older version>`.
set -euo pipefail
cd "$(dirname "$0")/.."

rev=${1:-HEAD}
min_age_days=7

# "name version" for every package in a lockfile, read from stdin.
packages() { awk -F'"' '/^name = / { name = $2 } /^version = / { print name, $2 }'; }

added=$(comm -13 <(git show "$rev:Cargo.lock" | packages | sort) <(packages <Cargo.lock | sort))
if [[ -z $added ]]; then
  echo "Cargo.lock adds no crates against $rev"
  exit 0
fi

young=0
cutoff=$(date -u -d "-$min_age_days days" +%Y-%m-%d)
while read -r name version; do
  # Workspace crates have no registry entry.
  [[ $name == uniwrt* ]] && continue
  published=$(curl -fsS -A 'uniwrt crate-age (github.com/robertzas/uniwrt)' \
    "https://crates.io/api/v1/crates/$name/$version" |
    python3 -c 'import json, sys; print(json.load(sys.stdin)["version"]["created_at"][:10])')
  flag=''
  if [[ $published > $cutoff ]]; then
    flag="  ← younger than $min_age_days days"
    young=$((young + 1))
  fi
  printf '%s  %s %s%s\n' "$published" "$name" "$version" "$flag"
done <<<"$added"

if ((young)); then
  printf '\n%s crate(s) inside the cooldown\n' "$young"
  exit 1
fi
