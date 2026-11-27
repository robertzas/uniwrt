# shellcheck shell=bash
# Helpers of the compatibility job's scripts (docs/features/lifecycle.md, Compatibility; D-152), sourced by
# write.sh and check.sh. A release's write.sh runs with the lib.sh of its own tag, so these may
# change: what the two sides share is the output directory and its manifest, `written.json`.

COMPAT_HTTP=${COMPAT_HTTP:-127.0.0.1:28080}
COMPAT_HTTPS=${COMPAT_HTTPS:-127.0.0.1:28443}
compat_pid=""
compat_jar=""

compat_log() { printf '%s\n' "$*" >&2; }
compat_fail() {
  printf 'compat: %s\n' "$*" >&2
  exit 1
}

# Starts `$1 serve` on data directory $2, its log in $3, and waits until it answers.
compat_start() {
  local bin=$1 data=$2 log=$3
  mkdir -p "$data"
  "$bin" serve --http-listen "$COMPAT_HTTP" --https-listen "$COMPAT_HTTPS" --data-dir "$data" \
    </dev/null >>"$log" 2>&1 &
  compat_pid=$!
  compat_jar=$(mktemp)
  for _ in $(seq 120); do
    if curl -fsSk -o /dev/null "https://$COMPAT_HTTPS/api/v1/health" 2>/dev/null; then return 0; fi
    if ! kill -0 "$compat_pid" 2>/dev/null; then
      compat_pid=""
      tail -n 40 "$log" >&2
      compat_fail "$bin exited before answering"
    fi
    sleep 0.5
  done
  tail -n 40 "$log" >&2
  kill "$compat_pid" 2>/dev/null || true
  compat_pid=""
  compat_fail "$bin didn't answer within 60 s"
}

# Stops the server compat_start started; a graceful shutdown exits 0.
compat_stop() {
  [[ -n $compat_pid ]] || return 0
  kill "$compat_pid"
  local status=0
  wait "$compat_pid" || status=$?
  compat_pid=""
  rm -f "$compat_jar"
  ((status == 0)) || compat_fail "the server exited with $status on SIGTERM"
}

# `api METHOD PATH [JSON]`: the response body on stdout; fails on an error status, printing it.
api() {
  local method=$1 path=$2 body=${3-} out status
  out=$(mktemp)
  local args=(-sSk -o "$out" -w '%{http_code}' -b "$compat_jar" -c "$compat_jar" -X "$method"
    -H 'accept: application/json')
  [[ $method == GET ]] || args+=(-H 'x-uniwrt-csrf: 1')
  [[ -z $body ]] || args+=(-H 'content-type: application/json' --data-binary "$body")
  status=$(curl "${args[@]}" "https://$COMPAT_HTTPS/api/v1$path")
  if ((status >= 400)); then
    compat_log "$method $path: HTTP $status: $(cat "$out")"
    rm -f "$out"
    return 1
  fi
  cat "$out"
  rm -f "$out"
}

# `api_file METHOD PATH FILE [JSON]`: the response body saved to FILE.
api_file() {
  local method=$1 path=$2 file=$3 body=${4-}
  local args=(-fsSk -o "$file" -b "$compat_jar" -c "$compat_jar" -X "$method")
  [[ $method == GET ]] || args+=(-H 'x-uniwrt-csrf: 1')
  [[ -z $body ]] || args+=(-H 'content-type: application/json' --data-binary "$body")
  curl "${args[@]}" "https://$COMPAT_HTTPS/api/v1$path"
}

# `login USER PASSWORD`
login() {
  api POST /auth/login "$(jq -nc --arg u "$1" --arg p "$2" '{username: $u, password: $p}')" >/dev/null
}
