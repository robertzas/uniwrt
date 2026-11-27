#!/usr/bin/env bash
# The compatibility job's write side (docs/features/lifecycle.md, Compatibility; D-152): a release's
# binary sets up a controller through its API, the data the promise keeps (the admin, a site
# revision with secrets, a notifier, the backup passphrase), then takes a controller backup and
# exports the site with its secrets. It writes them and `written.json` (what was written, for
# check.sh) to OUT, and leaves the data directory DATA for the upgrade in place.
#
# usage: write.sh BIN DATA OUT
#
# CI runs the copy from the older release's tag (its API matches its binary), so this script
# must keep working against the release it ships with; only the manifest is a contract.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
# shellcheck source=scripts/compat/lib.sh
source "$here/lib.sh"

(($# == 3)) || compat_fail "usage: write.sh BIN DATA OUT"
bin=$1 data=$2 out=$3
mkdir -p "$out"
trap 'compat_stop || true' EXIT

user=compat-admin
password=compat-admin-password-1x
backup_passphrase=compat-backup-passphrase
file_passphrase=compat-site-file-passphrase
wlan_id=compat
wlan_ssid='Compat Wi-Fi'
wlan_passphrase=compat-wlan-passphrase
mesh_key=0123456789abcdef0123456789abcdef
notifier='Compat ntfy'
ntfy_token=tk_compat_token_000000000000

compat_start "$bin" "$data" "$out/write.log"
version=$("$bin" version --json | jq -r .version)
compat_log "writing with uniwrt $version"

api POST /auth/setup "$(jq -nc --arg u "$user" --arg p "$password" '{username: $u, password: $p}')" >/dev/null
login "$user" "$password"

# A revision with secrets: a WLAN and the mesh with their keys.
site=$(api GET /site)
save=$(jq -c --arg id "$wlan_id" --arg ssid "$wlan_ssid" \
  --arg wpass "$wlan_passphrase" --arg mkey "$mesh_key" '{
  base: .revision.id,
  message: "Compat: a WLAN and the mesh",
  config: (.config
    | .wlans += [{
        id: $id, ssid: $ssid, enabled: true, network: "lan", security: "wpa2_wpa3",
        passphrase: null, bands: ["2g", "5g"], nodes: {scope: "all"}, hidden: false,
        isolation: false, max_clients: null, schedule: null,
        roaming: {fast: null, hints: true, steer: false}, multicast_to_unicast: true
      }]
    | .mesh.enabled = true),
  secrets: {("wlans/" + $id + "/passphrase"): $wpass, "mesh/key": $mkey}
}' <<<"$site")
revision=$(api PUT /site "$save" | jq -er .revision.id)

api POST /notifiers "$(jq -nc --arg name "$notifier" --arg token "$ntfy_token" '{
  name: $name, enabled: true,
  spec: {kind: "ntfy", server: "https://ntfy.invalid", topic: "uniwrt-compat", username: null},
  filter: {min: "warning", nodes: [], resolved: true, remind_s: null},
  secrets: {token: $token}
}')" >/dev/null

api PUT /controller/backups/passphrase "$(jq -nc --arg p "$backup_passphrase" '{passphrase: $p}')" >/dev/null
backup=$(api POST /controller/backups | jq -er .name)
api_file GET "/controller/backups/$backup" "$out/$backup"

api_file POST /site/export "$out/site.json" "$(jq -nc --arg p "$file_passphrase" '{passphrase: $p}')"

compat_stop
trap - EXIT

jq -n --arg version "$version" --arg user "$user" --arg password "$password" \
  --argjson revision "$revision" --arg wlan_id "$wlan_id" --arg ssid "$wlan_ssid" \
  --arg wpass "$wlan_passphrase" --arg mkey "$mesh_key" --arg notifier "$notifier" \
  --arg backup "$backup" --arg bpass "$backup_passphrase" --arg fpass "$file_passphrase" '{
  manifest: 1,
  uniwrt: $version,
  admin: {username: $user, password: $password},
  site: {
    revision: $revision,
    message: "Compat: a WLAN and the mesh",
    wlans: [{id: $wlan_id, ssid: $ssid}],
    mesh: true,
    secrets: {("wlans/" + $wlan_id + "/passphrase"): $wpass, "mesh/key": $mkey}
  },
  notifiers: [{name: $notifier, secrets: ["token"]}],
  backup: {file: $backup, passphrase: $bpass},
  site_file: {file: "site.json", passphrase: $fpass}
}' >"$out/written.json"
compat_log "wrote $out/written.json"
