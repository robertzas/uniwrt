#!/usr/bin/env bash
# The compatibility job's check side (docs/features/lifecycle.md, Compatibility; D-152): this release's
# binary opens what an older one wrote (write.sh, of the older release's tag) and must find in it
# everything `written.json` lists:
#
#   1. in place: the old data directory, upgraded at start (the admin's login, the site's latest
#      document and its secrets, the revision in history, the notifier, the backup passphrase,
#      which a new backup then uses);
#   2. restore: the old controller backup staged by `uniwrt restore` into a fresh directory;
#   3. import: the old site file with its secrets, onto a fresh controller.
#
# usage: check.sh BIN DATA OUT
#
# Items a newer write.sh adds to the manifest are checked once this script knows them; an older
# manifest's missing items are skipped (`// []`). The manifest's format only grows.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
# shellcheck source=scripts/compat/lib.sh
source "$here/lib.sh"

(($# == 3)) || compat_fail "usage: check.sh BIN DATA OUT"
bin=$1 data=$2 out=$3
manifest="$out/written.json"
[[ -f $manifest ]] || compat_fail "no $manifest"
jq -e '.manifest == 1' "$manifest" >/dev/null || compat_fail "unknown manifest format"
m() { jq -r "$1" "$manifest"; }
trap 'compat_stop || true' EXIT
compat_log "checking uniwrt $("$bin" version --json | jq -r .version) against $(m .uniwrt)'s data"

# The site's latest document holds the manifest's WLANs and mesh, and its secrets reveal.
check_site() {
  local site wlan id ssid path value got
  site=$(api GET /site)
  while IFS=$'\t' read -r id ssid; do
    wlan=$(jq -c --arg id "$id" '.config.wlans[] | select(.id == $id)' <<<"$site")
    [[ -n $wlan ]] || compat_fail "$1: WLAN $id is gone"
    [[ $(jq -r .ssid <<<"$wlan") == "$ssid" ]] || compat_fail "$1: WLAN $id's SSID changed"
  done < <(m '(.site.wlans // [])[] | [.id, .ssid] | @tsv')
  if [[ $(m '.site.mesh // false') == true ]]; then
    jq -e '.config.mesh.enabled' <<<"$site" >/dev/null || compat_fail "$1: the mesh is off"
  fi
  while IFS=$'\t' read -r path value; do
    got=$(api POST /site/secrets/reveal "$(jq -nc --arg p "$path" '{path: $p}')" | jq -r .value) ||
      compat_fail "$1: secret $path doesn't reveal"
    [[ $got == "$value" ]] || compat_fail "$1: secret $path changed"
  done < <(m '(.site.secrets // {}) | to_entries[] | [.key, .value] | @tsv')
  compat_log "ok    $1: the site's document and secrets"
}

# The revision the old release saved is in history, with its message.
check_history() {
  local id message
  id=$(m .site.revision) message=$(m .site.message)
  api GET '/revisions?limit=100' |
    jq -e --argjson id "$id" --arg message "$message" \
      'any(.revisions[]; .id == $id and .message == $message)' >/dev/null ||
    compat_fail "$1: revision $id isn't in history"
  compat_log "ok    $1: revision $id in history"
}

check_notifiers() {
  local list name secret
  list=$(api GET /notifiers)
  while IFS=$'\t' read -r name secret; do
    jq -e --arg name "$name" --arg secret "$secret" \
      'any(.notifiers[]; .name == $name and (.secrets[$secret] == true))' <<<"$list" >/dev/null ||
      compat_fail "$1: notifier $name or its $secret is gone"
  done < <(m '(.notifiers // [])[] | .name as $n | .secrets[] | [$n, .] | @tsv')
  compat_log "ok    $1: notifiers"
}

# The passphrase survived: the backup status says so, and a new backup is taken with it.
check_backups() {
  api GET /controller/backups | jq -e .passphrase_set >/dev/null ||
    compat_fail "$1: the backup passphrase is gone"
  api POST /controller/backups | jq -e .name >/dev/null || compat_fail "$1: no new backup"
  compat_log "ok    $1: the backup passphrase"
}

user=$(m .admin.username) password=$(m .admin.password)

# 1. In place.
compat_start "$bin" "$data" "$out/check-in-place.log"
login "$user" "$password" || compat_fail "in place: the admin can't log in"
check_site "in place"
check_history "in place"
check_notifiers "in place"
check_backups "in place"
compat_stop

# 2. Restore into a fresh directory.
if [[ $(m '.backup // empty') != "" ]]; then
  restored=$(mktemp -d)
  UNIWRT_BACKUP_PASSPHRASE=$(m .backup.passphrase) "$bin" restore "$out/$(m .backup.file)" \
    --data-dir "$restored" </dev/null >>"$out/check-restore.log" 2>&1 ||
    compat_fail "restore: \`uniwrt restore\` refused the backup (check-restore.log)"
  compat_start "$bin" "$restored" "$out/check-restore.log"
  login "$user" "$password" || compat_fail "restore: the admin can't log in"
  check_site "restore"
  check_history "restore"
  check_notifiers "restore"
  check_backups "restore"
  compat_stop
  rm -rf "$restored"
fi

# 3. Import the site file onto a fresh controller.
if [[ $(m '.site_file // empty') != "" ]]; then
  fresh=$(mktemp -d)
  compat_start "$bin" "$fresh" "$out/check-import.log"
  api POST /auth/setup "$(jq -nc --arg u "$user" --arg p "$password" '{username: $u, password: $p}')" >/dev/null
  login "$user" "$password"
  preview=$(api POST /site/import/preview "$(jq -c --arg p "$(m .site_file.passphrase)" \
    '{file: ., passphrase: $p}' "$out/$(m .site_file.file)")") ||
    compat_fail "import: the site file's preview was refused"
  jq -e 'all(.issues[]; .severity != "error")' <<<"$preview" >/dev/null ||
    compat_fail "import: the file has errors: $(jq -c '.issues' <<<"$preview")"
  jq -e 'all(.secrets[]; .source == "file" or .source == "generated")' <<<"$preview" >/dev/null ||
    compat_fail "import: secrets missing from the file: $(jq -c '.secrets' <<<"$preview")"
  api POST /site/import/accept "$(jq -c '{id, message: "Compat: the site file"}' <<<"$preview")" >/dev/null ||
    compat_fail "import: the accept was refused"
  check_site "import"
  compat_stop
  rm -rf "$fresh"
fi

trap - EXIT
compat_log "uniwrt $(m .uniwrt)'s data opens in this release"
