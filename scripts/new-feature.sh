#!/usr/bin/env bash
# Scaffolds a feature area along the feature recipe (CONVENTIONS.md), steps 2–6, wired in and
# compiling, with TODO(new-feature) markers where the real work goes (`just new-feature`):
#   model      crates/model/src/<mod>.rs, a type deriving TS behind the `ts` feature
#   api        crates/api/src/<mod>.rs, GET /api/v1/<area> answering a DTO that derives TS
#   web        web/src/features/<area>/<Pascal>Page.tsx and its test, the lazy route of a page
#              in app/nav.ts, the client in lib/api.ts and the strings in lib/strings.ts
#   telemetry  (--telemetry) node-agent/collect/<mod>.uc, its parser in crates/node/src/facts/
#              and a fixture; the steps that make it a collect set are printed
# Render (step 3) is skipped until crates/render exists. Nothing is written if any target
# exists already. Usage: scripts/new-feature.sh <area> [--path /page] [--telemetry]
set -euo pipefail
cd "$(dirname "$0")/.."

usage() {
  echo "usage: $0 <area> [--path /page] [--telemetry]" >&2
  echo "  <area>: lowercase words joined by hyphens, such as clients or floor-plans" >&2
  echo "  --path: the page's path in web/src/app/nav.ts (default: /<area>)" >&2
  exit 2
}

area='' path='' telemetry=''
while (($#)); do
  case $1 in
    --path)
      (($# > 1)) || usage
      path=$2
      shift
      ;;
    --telemetry) telemetry=1 ;;
    -h | --help) usage ;;
    -*) usage ;;
    *)
      [[ -z $area ]] || usage
      area=$1
      ;;
  esac
  shift
done
[[ $area =~ ^[a-z][a-z0-9]*(-[a-z0-9]+)*$ ]] || usage
path=${path:-/$area}
[[ $path =~ ^(/[a-z0-9-]+)+$ ]] || usage

# The area's names: mod (Rust module), Pascal (types, components), camel (TS keys), and the
# strings.nav key of the page, from its last path segment.
camel_of() { awk -F- '{ s = $1; for (i = 2; i <= NF; i++) s = s toupper(substr($i, 1, 1)) substr($i, 2); print s }' <<<"$1"; }
mod=${area//-/_}
camel=$(camel_of "$area")
pascal=${camel^}
nav_key=$(camel_of "${path##*/}")
dto=${pascal}Overview

model_file=crates/model/src/$mod.rs
api_file=crates/api/src/$mod.rs
page_dir=web/src/features/$area
page_file=$page_dir/${pascal}Page.tsx
test_file=$page_dir/$area.test.tsx
uc_file=node-agent/collect/$mod.uc
parser_file=crates/node/src/facts/$mod.rs
fixture_file=crates/node/fixtures/sets/$mod.json

## Conflicts: check everything before writing anything.

conflicts=()
targets=("$model_file" "$api_file" "$page_file" "$test_file" "web/src/api/gen/$dto.ts" "web/src/api/gen/$pascal.ts")
[[ -n $telemetry ]] && targets+=("$uc_file" "$parser_file" "$fixture_file")
for file in "${targets[@]}"; do [[ -e $file ]] && conflicts+=("$file exists"); done
grep -qE "^(pub )?mod $mod;" crates/model/src/lib.rs && conflicts+=("crates/model/src/lib.rs has mod $mod")
grep -qE "^(pub(\(crate\))? )?mod $mod;" crates/api/src/lib.rs && conflicts+=("crates/api/src/lib.rs has mod $mod")
grep -qF "\"/v1/$area\"" crates/api/src/lib.rs && conflicts+=("crates/api/src/lib.rs routes /v1/$area")
grep -qF "path: \"$path\"" web/src/app/nav.ts || conflicts+=("web/src/app/nav.ts has no page $path (add it there, or pass --path)")
grep -qF "\"$path\":" web/src/app/routes.tsx && conflicts+=("web/src/app/routes.tsx has a page for $path")
grep -qE "^  $camel: \{" web/src/lib/strings.ts && conflicts+=("web/src/lib/strings.ts has strings.$camel")
grep -qE "^    $nav_key: \{" web/src/lib/strings.ts || conflicts+=("web/src/lib/strings.ts has no strings.nav.$nav_key")
grep -qE "^  $camel:" web/src/lib/api.ts && conflicts+=("web/src/lib/api.ts has api.$camel")
if ((${#conflicts[@]})); then
  printf 'new-feature: nothing written, because:\n' >&2
  printf '  - %s\n' "${conflicts[@]}" >&2
  exit 1
fi

## Writing: templates use __mod__, __area__, __path__, __camel__, __Pascal__, __Dto__, __nav__.

created=() edited=()
render() {
  sed -e "s|__mod__|$mod|g; s|__area__|$area|g; s|__path__|$path|g; s|__camel__|$camel|g" \
    -e "s|__Pascal__|$pascal|g; s|__Dto__|$dto|g; s|__nav__|$nav_key|g"
}
create() {
  mkdir -p "$(dirname "$1")"
  render >"$1"
  created+=("$1")
}

# insert_sorted FILE PATTERN KEY_PREFIX KEY_SUFFIX KEY LINE: inserts LINE into the block of lines
# matching PATTERN, kept in the order of their keys (the line between the prefix and suffix
# regexes). Attribute lines (#[…]) stay with the line they precede.
insert_sorted() {
  # ENVIRON rather than -v, which would turn the regexes' backslashes into escapes.
  pattern=$2 prefix=$3 suffix=$4 key=$5 new=$6 awk '
    function emit_new() { if (!done) { print ENVIRON["new"]; done = 1 } }
    !done && $0 ~ ENVIRON["pattern"] {
      name = $0; sub(ENVIRON["prefix"], "", name); sub(ENVIRON["suffix"], "", name)
      seen = 1
      if (name > ENVIRON["key"]) emit_new()
      printf "%s", attrs; attrs = ""; print; next
    }
    !done && /^#\[/ { attrs = attrs $0 "\n"; next }
    { if (seen) emit_new(); printf "%s", attrs; attrs = ""; print }
    END { if (!done) exit 1 }' "$1" >"$1.new"
  mv "$1.new" "$1"
  edited+=("$1")
}

# insert_before FILE START ANCHOR TEXT: inserts TEXT before the first line matching ANCHOR that
# follows a line matching START, and before the comment lines right above that line.
insert_before() {
  start=$2 anchor=$3 text=$4 awk '
    !done && on && /^[ \t]*\/\// { held = held $0 "\n"; next }
    !done && on && $0 ~ ENVIRON["anchor"] { print ENVIRON["text"]; done = 1 }
    { printf "%s", held; held = "" }
    $0 ~ ENVIRON["start"] { on = 1 }
    { print }
    END { printf "%s", held; if (!done) exit 1 }' "$1" >"$1.new"
  mv "$1.new" "$1"
  edited+=("$1")
}

mod_line='^(pub(\([a-z]+\))? )?mod [a-z0-9_]+;'
mod_prefix='^(pub(\([a-z]+\))? )?mod '

# Step 2: model.
create "$model_file" <<'EOF'
//! The __area__ area's types (docs/features/). TODO(new-feature): what this module holds: desired
//! state, observed facts or both, and how they are validated.

use serde::{Deserialize, Serialize};

/// TODO(new-feature): the area's first type, with its real fields.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct __Pascal__ {
    pub names: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_json() {
        let value = __Pascal__ {
            names: vec!["example".to_owned()],
        };
        let json = serde_json::to_string(&value).unwrap();
        assert_eq!(serde_json::from_str::<__Pascal__>(&json).unwrap(), value);
    }
}
EOF
insert_sorted crates/model/src/lib.rs "$mod_line" "$mod_prefix" ';.*' "$mod" "pub mod $mod;"

# Step 5: API.
create "$api_file" <<'EOF'
//! The __area__ routes, with a session: `GET /api/v1/__area__`. TODO(new-feature): what they
//! serve and where it comes from (the live fleet, the store or a job).

use axum::{Json, extract::State};
use serde::Serialize;
use ts_rs::TS;
use uniwrt_model::__mod__::__Pascal__;

use crate::{Api, ApiResult, CurrentUser};

/// What `GET /api/v1/__area__` answers.
#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub(crate) struct __Dto__ {
    pub(crate) __mod__: __Pascal__,
}

/// TODO(new-feature): read the area's state from `api`.
pub(crate) async fn get(State(_api): State<Api>, _: CurrentUser) -> ApiResult<Json<__Dto__>> {
    Ok(Json(__Dto__ {
        __mod__: __Pascal__::default(),
    }))
}

#[cfg(test)]
mod tests {
    use axum::{body::Body, extract::Request, http::StatusCode};

    use crate::testing::{Demo, json};

    #[tokio::test]
    async fn the_overview_needs_a_session() {
        let demo = Demo::start().await;
        let anonymous = Request::get("/v1/__area__").body(Body::empty()).unwrap();
        assert_eq!(
            demo.send(anonymous).await.status(),
            StatusCode::UNAUTHORIZED
        );
        let res = demo.send(demo.get("/v1/__area__")).await;
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(
            json(res).await,
            serde_json::json!({ "__mod__": { "names": [] } })
        );
    }
}
EOF
insert_sorted crates/api/src/lib.rs "$mod_line" "$mod_prefix" ';.*' "$mod" "mod $mod;"
insert_before crates/api/src/lib.rs '^pub fn router' '\.route\("/v1/ws"' \
  "        .route(\"/v1/$area\", get($mod::get))"

# Step 6: UI.
create "$page_file" <<'EOF'
import { useQuery } from "@tanstack/solid-query";
import { createEffect, Loading, Match, Switch } from "solid-js";
import { api } from "../../lib/api";
import { strings } from "../../lib/strings";
import { Card, CardHeader } from "../../ui/Card";

const s = strings.__camel__;
const page = strings.nav.__nav__;

/** The __Pascal__ page. TODO(new-feature): what it shows, built from ui/ primitives. */
export function __Pascal__Page() {
  const overview = useQuery(() => ({
    queryKey: ["__area__"],
    queryFn: ({ signal }) => api.__camel__(signal),
  }));

  createEffect(
    () => page.label,
    (label) => {
      document.title = `${label} · ${strings.appName}`;
    },
  );

  const note = (text: string) => (
    <p role="status" class="px-5 py-10 text-center text-fg-subtle text-sm">
      {text}
    </p>
  );

  return (
    <Card class="mx-auto max-w-7xl overflow-hidden">
      <CardHeader title={page.label} description={page.description} />
      <Switch fallback={note(`${s.loading}…`)}>
        <Match when={overview.isError}>{note(s.failed)}</Match>
        <Match when={overview.isSuccess}>
          <Loading fallback={note(`${s.loading}…`)}>
            {note(s.count(overview.data?.__mod__.names.length ?? 0))}
          </Loading>
        </Match>
      </Switch>
    </Card>
  );
}
EOF
create "$test_file" <<'EOF'
import { render, screen } from "@solidjs/testing-library";
import { QueryClient, QueryClientProvider } from "@tanstack/solid-query";
import { describe, expect, it, vi } from "vitest";
import type { __Dto__ } from "../../api/gen/__Dto__";
import { __Pascal__Page } from "./__Pascal__Page";

function renderPage() {
  render(() => (
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <__Pascal__Page />
    </QueryClientProvider>
  ));
}

describe("__Pascal__Page", () => {
  it("shows what the server reports", async () => {
    const overview: __Dto__ = { __mod__: { names: ["example"] } };
    const fetch = vi.fn(async () => Response.json(overview));
    vi.stubGlobal("fetch", fetch);
    renderPage();
    await screen.findByText("1 item");
    expect(fetch).toHaveBeenCalledWith("/api/v1/__area__", expect.anything());
  });

  it("says when the server cannot be reached", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(async () => new Response(null, { status: 503 })),
    );
    renderPage();
    await screen.findByText(/couldn't load/i);
  });
});
EOF
insert_before web/src/app/routes.tsx '^const pages:' '^};$' "$(render <<'EOF'
  "__path__": lazy(() =>
    import("../features/__area__/__Pascal__Page").then((m) => ({ default: m.__Pascal__Page })),
  ),
EOF
)"
insert_before web/src/lib/api.ts '^export const api = \{' '^};$' "$(render <<'EOF'
  /** TODO(new-feature): what `GET /api/v1/__area__` answers. */
  __camel__: (signal?: AbortSignal) => request<__Dto__>("/__area__", { signal }),
EOF
)"
insert_before web/src/lib/api.ts '' '^import ' "import type { $dto } from \"../api/gen/$dto\";"
insert_before web/src/lib/strings.ts '^export const strings = \{' '^\} as const;$' "$(render <<'EOF'
  __camel__: {
    loading: "Loading",
    failed: "Couldn't load this page. Try again in a moment.",
    // TODO(new-feature): the page's own strings.
    count: (count: number) => `${count} ${count === 1 ? "item" : "items"}`,
  },
EOF
)"

# Step 4: telemetry, on request.
if [[ -n $telemetry ]]; then
  create "$uc_file" <<'EOF'
// The `__mod__` set: TODO(new-feature): the raw sources it gathers, which
// crates/node/src/facts/__mod__.rs parses.

function collect() {
	return { names: [] };
}

export { collect };
EOF
  create "$parser_file" <<'EOF'
//! The `__mod__` set: TODO(new-feature): the raw sources that `node-agent/collect/__mod__.uc`
//! gathers, and how they become the model's types.
#![cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "TODO(new-feature): parse it in SetFacts, with a variant of proto::ops::Set"
    )
)]

use serde::Deserialize;
use uniwrt_model::__mod__::__Pascal__;

#[derive(Deserialize)]
pub(super) struct Raw {
    #[serde(default)]
    names: Vec<String>,
}

pub(super) fn parse(raw: Raw) -> __Pascal__ {
    __Pascal__ { names: raw.names }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_fixture() {
        let raw: Raw =
            serde_json::from_str(include_str!("../../fixtures/sets/__mod__.json")).unwrap();
        assert_eq!(parse(raw).names, ["example"]);
    }
}
EOF
  create "$fixture_file" <<'EOF'
{ "names": ["example"] }
EOF
  insert_sorted crates/node/src/facts/mod.rs "$mod_line" "$mod_prefix" ';.*' "$mod" "mod $mod;"
  insert_sorted crates/node/src/bundle.rs '^    file!\("collect/' '^    file!\("collect/' '\.uc"\),$' \
    "$mod" "    file!(\"collect/$mod.uc\"),"
  # The bundle's array length counts its files.
  awk '/^const FILES: \[\(&str, &str\); [0-9]+\] = \[$/ {
      n = $0; gsub(/[^0-9]/, "", n); sub(/; [0-9]+\]/, "; " n + 1 "]")
    } { print }' crates/node/src/bundle.rs >crates/node/src/bundle.rs.new
  mv crates/node/src/bundle.rs.new crates/node/src/bundle.rs
fi

## Formatting and the generated types.

cargo fmt --all
mapfile -t web_files < <(printf '%s\n' "${created[@]}" "${edited[@]}" |
  sed -n 's|^web/||p' | grep -E '\.tsx?$' | sort -u)
(cd web && node_modules/.bin/biome check --write "${web_files[@]}" >/dev/null 2>&1)
just --quiet gen >/dev/null

printf 'new-feature %s: created\n' "$area"
printf '  %s\n' "${created[@]}"
printf 'edited\n'
printf '%s\n' "${edited[@]}" | sort -u | sed 's/^/  /'
cat <<EOF
next
  - spec: the area's section in docs/features/<area>.md, and any decision in docs/DECISIONS.md
  - render: skipped until crates/render exists (step 3 of the recipe)
EOF
if [[ -n $telemetry ]]; then
  cat <<EOF
  - telemetry: make $mod a collect set: a variant of proto::ops::Set (crates/node/src/proto/ops.rs)
    and its line in node-agent/collect.uc's SETS; a SetFacts variant in crates/node/src/facts/mod.rs
    and the NodeFacts field it fills; then drop the dead_code expectation in $parser_file
EOF
fi
cat <<EOF
  - grep -rn 'TODO(new-feature)' for the placeholders, then \`just check\`
EOF
