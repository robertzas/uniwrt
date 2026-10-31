#!/usr/bin/env bash
# Tests scripts/arch-check.sh against a small fixture workspace, one violation per case;
# `just check` and CI run it.
set -euo pipefail

root=$(cd "$(dirname "$0")/.." && pwd)
script=$root/scripts/arch-check.sh
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
failures=0

# put FILE CONTENT: writes a fixture file, creating its directory.
put() {
  mkdir -p "$(dirname "$1")"
  printf '%s\n' "$2" >"$1"
}

# krate DIR [DEPENDENCIES [DEV-DEPENDENCIES]]: a library crate named uniwrt-DIR.
krate() {
  put "crates/$1/Cargo.toml" "[package]
name = \"uniwrt-$1\"
version = \"0.0.0\"
edition = \"2024\"

[dependencies]
${2:-}

[dev-dependencies]
${3:-}"
  put "crates/$1/src/lib.rs" "//! The $1 crate."
}

# shellcheck disable=SC2016 # the backticks are Markdown
row() { printf '| %s | `%s` | – | %s |\n' "$1" "$2" "$3"; }

table() {
  put docs/ARCHITECTURE.md "# Architecture

## Crates and layering

| Layer | Crate | Responsibility | May depend on |
|---|---|---|---|
$(row 'L0 pure' core '—')
$(row L0 model core)
$(row 'L2 services' engine 'L0')
$(row test sim 'core, model')
${1:-}

Rules: none here.

## Next"
}

# The clean fixture: every rule holds, including the allowed exceptions.
fixture() {
  rm -rf "$tmp/ws"
  mkdir -p "$tmp/ws"
  cd "$tmp/ws"
  cp "$root/rust-toolchain.toml" .
  put Cargo.toml '[workspace]
resolver = "3"
members = ["crates/*"]
exclude = ["vendor"]'
  put vendor/tokio/Cargo.toml '[package]
name = "tokio"
version = "1.0.0"
edition = "2024"'
  put vendor/tokio/src/lib.rs ''
  table
  krate core
  put crates/core/src/clock.rs 'use std::time::SystemTime;
pub fn now() -> SystemTime {
    SystemTime::now() // arch-check: allow (the one wall clock)
}
// std::fs in a comment is fine.'
  krate model 'uniwrt-core = { path = "../core" }'
  krate engine 'uniwrt-core = { path = "../core" }
tokio = { path = "../../vendor/tokio" }' 'uniwrt-sim = { path = "../sim" }'
  krate sim 'uniwrt-core = { path = "../core" }' 'uniwrt-engine = { path = "../engine" }'
  put web/src/ui/Button.tsx 'import { strings } from "../lib/strings";'
  put web/src/lib/strings.ts 'export const strings = {};'
  put web/src/lib/api.ts 'import type { Health } from "../api/gen/Health";'
  put web/src/api/gen/Health.ts 'export type Health = {};'
  put web/src/app/routes.tsx 'const page = () => import("../features/a/APage");'
  put web/src/features/a/APage.tsx 'import { Button } from "../../ui/Button";
import { strings } from "../../lib/strings";
import { Row } from "./parts/Row";'
  put web/src/features/a/parts/Row.tsx 'import { api } from "../../../lib/api";'
  put web/src/features/a/a.test.tsx 'import { createAppRouter } from "../../app/routes";'
}

# run NAME EXPECTED: runs the check; EXPECTED is "pass" or a regex its failure must print.
run() {
  local out status=0
  cargo generate-lockfile --offline --quiet
  out=$("$script" "$PWD" 2>&1) || status=$?
  if [[ $2 == pass && $status == 0 ]] || [[ $2 != pass && $status != 0 && $out =~ $2 ]]; then
    printf 'ok    %s\n' "$1"
  else
    printf 'FAIL  %s (exit %s, expected %s):\n%s\n' "$1" "$status" "$2" "$out"
    failures=$((failures + 1))
  fi
}

fixture
run 'the clean fixture passes' pass

fixture
krate model 'uniwrt-engine = { path = "../engine" }'
run 'a dependency on a higher layer fails' 'crates/model \(L0\) depends on engine \(L2\) as a normal dependency'

fixture
krate extra
run 'a crate missing from the table fails' 'crates/extra is not in docs/ARCHITECTURE.md'

fixture
put crates/extra/Cargo.toml '[package]
name = "extra"
version = "0.0.0"
edition = "2024"'
put crates/extra/src/lib.rs ''
table "$(row L1 extra core)"
run 'a package not named uniwrt-<dir> fails' 'the package is extra, not uniwrt-extra'

fixture
krate model 'uniwrt-core = { path = "../core" }
tokio = { path = "../../vendor/tokio" }'
run 'tokio in an L0 crate fails' 'crates/model \(L0\) depends on tokio'

fixture
krate core '' 'uniwrt-model = { path = "../model" }'
run 'an L0 dev-dependency outside the table fails' 'depends on model \(L0\) as a dev dependency'

fixture
put crates/model/src/io.rs 'pub fn read() -> String { std::fs::read_to_string("x").unwrap() }'
run 'std::fs in an L0 crate fails' 'io.rs:1: IO through std'

fixture
put crates/model/src/io.rs 'use std::{
    fmt,
    fs,
};'
run 'a grouped import of std::fs fails' 'io.rs:3: IO through std'

fixture
put crates/core/src/wait.rs 'pub fn now() -> std::time::Instant { std::time::Instant::now() }'
run 'a clock read in an L0 crate fails' 'wait.rs:1: a clock read'

fixture
put crates/model/tests/print.rs '#[test]
fn shows() { println!("hi"); }'
run 'printing in an L0 crate test fails' 'print.rs:2: terminal IO'

fixture
table "$(row L1 store 'core, nosuch')"
run 'an unknown name in the table fails' 'store may depend on "nosuch", which is neither'

fixture
table "$(row L1 store 'core, engine')"
run 'a table row allowing a higher layer fails' 'store \(L1\) may depend on engine \(L2\), a higher layer'

fixture
put web/src/features/b/BPage.tsx 'import { APage } from "../a/APage";'
run 'a feature importing another feature fails' 'features/b/BPage.tsx:1: imports features/a/APage'

fixture
put web/src/features/a/APage.tsx 'import { routes } from "../../app/routes";'
run 'a feature importing app/ outside its tests fails' 'APage.tsx:1: imports app/routes'

fixture
put web/src/ui/Table.tsx 'import { api } from "../lib/api";'
run 'ui/ importing lib/api fails' 'ui/Table.tsx:1: imports lib/api \(ui/ imports only lib/strings'

fixture
put web/src/lib/live.ts 'import { Button } from "../ui/Button";'
run 'lib/ importing ui/ fails' 'lib/live.ts:1: imports ui/Button \(lib/ imports only api/\)'

if ((failures)); then
  printf '%d failure(s)\n' "$failures"
  exit 1
fi
echo 'all arch-check tests passed'
