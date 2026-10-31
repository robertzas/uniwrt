#!/usr/bin/env bash
# Enforces the layering rules (`just arch-check`; `just check` and CI run it):
#   - each crate depends only on the crates that docs/ARCHITECTURE.md's table ("Crates and
#     layering") allows, read from the doc itself so the two cannot drift; dev-dependencies may
#     also use the test crates, whose own dev-dependencies are free;
#   - L0 crates have no async runtime, socket, database or SSH crate in their dependency tree,
#     and their source does no IO and reads no clock (CONVENTIONS.md, Architecture rules); a
#     line marked `arch-check: allow` is exempt, with the reason beside the marker;
#   - in web/src, features never import other features, ui/ imports only lib/strings from
#     outside itself, lib/ imports only api/, and only a feature's tests import app/.
# Usage: scripts/arch-check.sh [root]; scripts/arch-check.test.sh runs it on fixture trees.
set -euo pipefail
cd "${1:-$(dirname "$0")/..}"

errors=0
fail() {
  printf '✗ %s\n' "$*"
  errors=$((errors + 1))
}

# Crates an L0 crate must not reach, even transitively.
impure=(tokio mio socket2 async-std smol rusqlite libsqlite3-sys russh hyper axum reqwest ureq)

## Crates: the table, as "crate layer allowed" lines (layer is L0…L3 or test).

table=$(awk -F'|' '
  /^## Crates and layering/ { on = 1; next }
  /^## / { on = 0 }
  on && /^\| (L[0-9]|test)/ {
    split($2, layer, " "); split($3, name, "`"); allowed = $5
    crate = name[2]; gsub(/^ +| +$/, "", allowed)
    print crate "\t" layer[1] "\t" allowed
  }' docs/ARCHITECTURE.md)
[[ -n $table ]] || {
  fail 'docs/ARCHITECTURE.md has no table under "## Crates and layering"'
  exit 1
}

declare -A layer_of allowed_raw
crates=()
while IFS=$'\t' read -r crate layer allowed; do
  layer_of[$crate]=$layer
  allowed_raw[$crate]=$allowed
  crates+=("$crate")
done <<<"$table"

rank() { [[ $1 == test ]] && echo 9 || echo "${1#L}"; }

# Expands a "May depend on" cell: "—", "all", crate names, layers ("L1") and ranges ("L0–L2").
# An item that is none of these comes out as "!item".
expand() {
  local cell=$1 item from to c
  local -a items
  [[ $cell == "—" || $cell == "-" || -z $cell ]] && return 0
  IFS=',' read -ra items <<<"$cell"
  for item in "${items[@]}"; do
    item=${item// /}
    if [[ $item == all ]]; then
      printf '%s\n' "${crates[@]}"
    elif [[ $item =~ ^L([0-9])(–|-)L([0-9])$ ]]; then
      from=${BASH_REMATCH[1]} to=${BASH_REMATCH[3]}
      for c in "${crates[@]}"; do
        [[ ${layer_of[$c]} != test ]] && ((${layer_of[$c]#L} >= from && ${layer_of[$c]#L} <= to)) && echo "$c"
      done
    elif [[ $item =~ ^L[0-9]$ ]]; then
      for c in "${crates[@]}"; do [[ ${layer_of[$c]} == "$item" ]] && echo "$c"; done
    elif [[ -n ${layer_of[$item]:-} ]]; then
      echo "$item"
    else
      echo "!$item"
    fi
  done
  return 0
}

declare -A allowed
for crate in "${crates[@]}"; do
  allowed[$crate]=" $(expand "${allowed_raw[$crate]}" | tr '\n' ' ') "
  # The table itself: no crate may be allowed a higher layer (the test crates are the documented
  # exception: server runs sim for demo mode).
  for dep in ${allowed[$crate]}; do
    if [[ $dep == '!'* ]]; then
      fail "docs/ARCHITECTURE.md: $crate may depend on \"${dep#!}\", which is neither a crate nor a layer"
      continue
    fi
    [[ ${layer_of[$crate]} == test || ${layer_of[$dep]} == test ]] && continue
    if (($(rank "${layer_of[$dep]}") > $(rank "${layer_of[$crate]}"))); then
      fail "docs/ARCHITECTURE.md: $crate (${layer_of[$crate]}) may depend on $dep (${layer_of[$dep]}), a higher layer"
    fi
  done
done

## Crates: the manifests, as "dir package kind dependency" lines (kind: normal, build or dev).

metadata=$(cargo metadata --format-version 1 --no-deps --locked)
edges=$(jq -r '
  .packages[]
  | (.manifest_path | split("/") | .[-2]) as $dir
  | ([$dir, .name, "-", "-"] | @tsv),
    (.dependencies[] | select(.name | startswith("uniwrt-"))
      | [$dir, "-", (.kind // "normal"), (.name | ltrimstr("uniwrt-"))] | @tsv)' <<<"$metadata")

checked=0
declare -A present
while IFS=$'\t' read -r dir package kind dep; do
  if [[ $kind == - ]]; then
    present[$dir]=1
    checked=$((checked + 1))
    # The binary's package is plain `uniwrt` (ARCHITECTURE.md: "server (bin uniwrt)").
    [[ $package == "uniwrt-$dir" || ($dir == server && $package == uniwrt) ]] ||
      fail "crates/$dir: the package is $package, not uniwrt-$dir"
    [[ -n ${layer_of[$dir]:-} ]] || fail "crates/$dir is not in docs/ARCHITECTURE.md's table"
    continue
  fi
  [[ -n ${layer_of[$dir]:-} ]] || continue
  [[ ${allowed[$dir]} == *" $dep "* ]] && continue
  if [[ $kind == dev ]] && [[ ${layer_of[$dir]} == test || ${layer_of[$dep]:-} == test ]]; then
    continue
  fi
  fail "crates/$dir (${layer_of[$dir]}) depends on $dep (${layer_of[$dep]:-not in the table}) as a $kind dependency; it may use: ${allowed_raw[$dir]}"
done <<<"$edges"

## L0 purity: the dependency tree and the source.

impure_re="^($(IFS='|' && echo "${impure[*]}"))$"
for crate in "${crates[@]}"; do
  [[ ${layer_of[$crate]} == L0 && -n ${present[$crate]:-} ]] || continue
  while read -r name; do
    fail "crates/$crate (L0) depends on $name; see \`cargo tree -p uniwrt-$crate -e normal -i $name\`"
  done < <(cargo tree --locked -p "uniwrt-$crate" -e normal --all-features --target all \
    --prefix none --format '{p}' | awk '{ print $1 }' | grep -E "$impure_re" | sort -u || true)

  mapfile -t sources < <(find "crates/$crate" -name '*.rs' -not -path '*/target/*' | sort)
  ((${#sources[@]})) || continue
  while IFS= read -r hit; do
    fail "$hit"
  done < <(awk '
    BEGIN { B = "(^|[^A-Za-z0-9_])"; E = "([^A-Za-z0-9_]|$)" }
    FNR == 1 { grouped = 0 }
    /arch-check: allow/ { next }
    {
      line = $0
      sub(/^[ \t]+/, "", line)
      if (line ~ /^\/\//) next
      sub(/[ \t]*\/\/.*$/, "", line)
      if (line ~ /^(pub(\([a-z]+\))? )?use std::\{/) grouped = 1
      why = ""
      if (line ~ B "std::(fs|process|env|thread|os)" E) why = "IO through std"
      else if (grouped && line ~ B "(fs|process|env|thread|os)" E) why = "IO through std"
      else if (line ~ B "(TcpStream|TcpListener|UdpSocket)" E) why = "a socket"
      else if (line ~ B "std::io::(stdin|stdout|stderr)" E || line ~ B "e?print(ln)?!") why = "terminal IO"
      else if (line ~ B "(SystemTime|Instant|Utc|Local)::now" E || line ~ /OffsetDateTime::now/) \
        why = "a clock read (inject a Clock)"
      if (grouped && line ~ /;/) grouped = 0
      if (why != "") printf "%s:%d: %s (L0 crates are pure): %s\n", FILENAME, FNR, why, line
    }' "${sources[@]}")
done

## Web: the import directions in web/src (generated types excluded).

# Joins a directory and a relative specifier into a path from web/src, resolving . and ..
resolve() {
  local -a parts out=()
  local part
  IFS=/ read -ra parts <<<"$1/$2"
  for part in "${parts[@]}"; do
    case $part in
      '' | .) ;;
      ..) ((${#out[@]})) && unset 'out[-1]' ;;
      *) out+=("$part") ;;
    esac
  done
  (IFS=/ && echo "${out[*]}")
}

# The area of a path from web/src: features/<name>, or its first directory, or "root".
area() {
  case $1 in
    features/*/*) local rest=${1#features/} && echo "features/${rest%%/*}" ;;
    */*) echo "${1%%/*}" ;;
    *) echo root ;;
  esac
}

imports=0
if [[ -d web/src ]]; then
  while IFS=: read -r file line spec; do
    imports=$((imports + 1))
    spec=${spec#*\"} spec=${spec%\"}
    from=${file#web/src/}
    to=$(resolve "$(dirname "$from")" "$spec")
    src=$(area "$from") dst=$(area "$to")
    [[ $src == "$dst" ]] && continue
    ok=
    case $src in
      features/*)
        case $dst in
          api | lib | ui | test) ok=1 ;;
          app) [[ $from == *.test.ts || $from == *.test.tsx ]] && ok=1 ;;
          features/*) ;;
        esac
        why="features never import other features, and only their tests import app/"
        ;;
      ui)
        [[ $dst == test || $to == lib/strings || $to == lib/strings.ts ]] && ok=1
        why="ui/ imports only lib/strings from outside itself"
        ;;
      lib)
        [[ $dst == api || $dst == test ]] && ok=1
        why="lib/ imports only api/"
        ;;
      api)
        why="api/ imports nothing else"
        ;;
      dev)
        [[ $dst == ui || $dst == lib || $dst == api || $dst == test ]] && ok=1
        why="dev/ imports only ui/, lib/ and api/"
        ;;
      *) ok=1 ;;
    esac
    [[ -n $ok ]] || fail "$file:$line: imports $to ($why)"
  done < <(grep -rnoE --include='*.ts' --include='*.tsx' --exclude-dir=gen \
    '(from|import\(?) *"\.\.?/[^"]*"' web/src || true)
fi

if ((errors)); then
  printf '\narch-check failed: %d violation(s)\n' "$errors"
  exit 1
fi
printf 'arch-check passed: %d crates, %d web imports\n' "$checked" "$imports"
