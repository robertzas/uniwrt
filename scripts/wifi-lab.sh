#!/usr/bin/env bash
# The hwsim lab (TESTING.md, Wi-Fi lab): OpenWrt nodes with virtual radios (mac80211_hwsim) and a
# batman-adv mesh, in containers booted with procd. Node a (25.12) is the gateway, with APs on 2g
# and 5g, a mesh point and DHCP on the mesh. Node b (24.10) meshes with a over Wi-Fi and over a
# wired backhaul (`bh0`, a veth pair with batman's MTU of 1532). Client c (25.12) joins a's 2g AP
# and b's 5g AP. Each role's config is in scripts/wifi-lab/<role>/.
#
# The host's kernel provides the radios and batman-adv. Loading them and moving radios into the
# containers (and linking them) needs root, which a privileged helper container provides (D-035);
# only hwsim radios are ever moved. `down` unloads the modules if `up` loaded them.
#
# Usage: scripts/wifi-lab.sh up | down | test [nextest args…] | capture [collect | events]
#   up       builds the images, starts the lab and writes target/wifi-lab.env (addresses, password)
#   test     runs crates/node/tests/wifi.rs against a running lab, starting it first if needed
#   capture  (both parts unless one is named) writes the collect answers of nodes a and b, from a running lab, to
#            crates/node/fixtures/collect/hwsim-<role>-<release>.json, and a's neighbors set
#            with and without rtnetlink to hwsim-a-<release>-neighbors.json; and what a's agent
#            pushes while the client is kicked and a radio restarts, to
#            crates/node/fixtures/events/hwsim-a-<release>.jsonl
set -euo pipefail
cd "$(dirname "$0")/.."

ROLES=(a b c)
declare -A IMAGE=([a]=25.12.6 [b]=24.10.8 [c]=25.12.6)
# The hwsim radios of each role, by index (hwsimN is radio N's device).
declare -A RADIOS=([a]="0 1" [b]="2 3" [c]="4 5")
HWSIM_RADIOS=6
ENV_FILE=target/wifi-lab.env
LOADED=target/wifi-lab.loaded
HELPER_IMAGE=openwrt/rootfs:x86-64-25.12.6

# Runs a command as root on the host: in a privileged container that shares the host's network
# and PID namespaces and chroots into its filesystem.
host_root() {
  docker run --rm --name uniwrt-root-helper --privileged --net host --pid host -v /:/host \
    "$HELPER_IMAGE" chroot /host /bin/sh -c "$@"
}

hwsim_phy() { ls "/sys/devices/virtual/mac80211_hwsim/hwsim$1/ieee80211"; }

modules() {
  mkdir -p target # a fresh checkout has none
  # Without the regulatory database the kernel keeps the world domain, where channel 36 is
  # no-initiate-radiation, and the 5 GHz APs and the mesh never start.
  if [[ ! -e /lib/firmware/regulatory.db ]]; then
    echo "the lab needs the kernel's regulatory database: install wireless-regdb, then run 'iw reg reload' as root" >&2
    exit 1
  fi
  if [[ ! -d /sys/module/mac80211_hwsim || ! -d /sys/module/batman_adv ]]; then
    [[ -d /sys/module/mac80211_hwsim ]] || touch "$LOADED"
    host_root "modprobe mac80211_hwsim radios=$HWSIM_RADIOS && modprobe batman-adv"
  fi
  local count
  count=$(find /sys/devices/virtual/mac80211_hwsim -maxdepth 1 -name 'hwsim*' | wc -l)
  if ((count < HWSIM_RADIOS)); then
    echo "mac80211_hwsim has $count radios; the lab needs $HWSIM_RADIOS (unload it first)" >&2
    exit 1
  fi
}

images() {
  for version in $(printf '%s\n' "${IMAGE[@]}" | sort -u); do
    docker build -q -f scripts/wifi-node.Dockerfile --build-arg "IMAGE=openwrt/rootfs:x86-64-$version" \
      -t "uniwrt-wifi-node:$version" scripts >/dev/null
  done
}

# Runs as PID 1 before procd: sets the root password ($0), installs the role's wireless config,
# leaves a uci-defaults script that applies the role's setup.uci after the first boot has
# generated the network config (and keeps the container's address on lan, as test-agent.sh
# does), then waits for the role's $1 radios to arrive.
# shellcheck disable=SC2016 # expanded by the container's shell
SETUP='
  printf "%s\n%s\n" "$0" "$0" | passwd root >/dev/null 2>&1
  cp /lab/wireless /etc/config/wireless
  address=$(ip -4 -o addr show dev eth0 | awk "{ print \$4 }")
  printf "%s\n" "uci set network.lan.ipaddr=$address" "uci -q delete network.lan.netmask" \
    "uci -q delete network.wan; uci -q delete network.wan6" "uci batch </lab/setup.uci" \
    "uci commit" >/etc/uci-defaults/99-uniwrt-lab
  while [ "$(ls /sys/class/ieee80211 | wc -l)" -lt "$1" ]; do sleep 1; done
  exec /sbin/init'

# Ready when the client has addresses on both uplinks: c's 5g uplink reaches a's DHCP server
# through b and the mesh.
# shellcheck disable=SC2016
READY='for i in wwan wwan5; do ubus -S call network.interface.$i status | grep -q "\"ipv4-address\":\[{" || exit 1; done'
# The gateway is ready when its 802.11s peer link with b is established and batman sees b over
# both the air and the backhaul: the client's addresses can arrive over the backhaul alone.
# shellcheck disable=SC2016
READY_GW='mesh=$(iw dev | awk "/Interface/ { i = \$2 } /type mesh point/ { print i }")
  [ -n "$mesh" ] && iw dev "$mesh" station dump | grep -q "mesh plink:.*ESTAB" &&
  [ "$(batctl meshif bat0 n -H | wc -l)" -ge 2 ]'

up() {
  modules
  images
  down_containers
  local password moves='' role
  declare -A pid
  password="uniwrt-$(od -An -N8 -tx8 /dev/urandom | tr -d ' ')"
  for role in "${ROLES[@]}"; do
    local radios
    read -ra radios <<<"${RADIOS[$role]}"
    # SYS_PTRACE: netifd checks the Wi-Fi daemons it tracks through /proc/<pid>/exe, and fails
    # the radio without it.
    docker create --name "uniwrt-wifi-$role" --cap-add NET_ADMIN --cap-add SYS_PTRACE \
      -p 127.0.0.1::22 \
      -v "$PWD/scripts/wifi-lab/$role:/lab:ro" "uniwrt-wifi-node:${IMAGE[$role]}" \
      /bin/sh -c "$SETUP" "$password" "${#radios[@]}" >/dev/null
    docker start "uniwrt-wifi-$role" >/dev/null
    pid[$role]=$(docker inspect -f '{{.State.Pid}}' "uniwrt-wifi-$role")
    local index
    for index in "${radios[@]}"; do
      moves+="iw phy $(hwsim_phy "$index") set netns ${pid[$role]} && "
    done
  done
  # The backhaul first: the containers boot once their radios have arrived.
  host_root "ip link add bh0 mtu 1532 netns ${pid[a]} type veth peer name bh0 mtu 1532 \
    netns ${pid[b]} && ${moves}true"
  for _ in $(seq 120); do
    if docker exec uniwrt-wifi-c /bin/sh -c "$READY" 2>/dev/null &&
      docker exec uniwrt-wifi-a /bin/sh -c "$READY_GW" 2>/dev/null; then
      local port_a port_b
      port_a=$(docker port uniwrt-wifi-a 22/tcp | head -n1 | sed 's/.*://')
      port_b=$(docker port uniwrt-wifi-b 22/tcp | head -n1 | sed 's/.*://')
      printf '%s\n' "UNIWRT_LAB_GW=127.0.0.1:$port_a" "UNIWRT_LAB_AP=127.0.0.1:$port_b" \
        "UNIWRT_LAB_PASSWORD=$password" >"$ENV_FILE"
      echo "the lab is up ($ENV_FILE)"
      return
    fi
    sleep 1
  done
  echo "the lab did not come up: client c has no addresses, or a's mesh is not up" >&2
  diagnose >&2
  exit 1
}

# What each node's radios, mesh and uplinks look like, for a lab that did not come up.
diagnose() {
  local role
  for role in "${ROLES[@]}"; do
    echo "== $role"
    # shellcheck disable=SC2016 # expanded by the container's shell
    docker exec "uniwrt-wifi-$role" /bin/sh -c '
      iw dev; ubus call network.wireless status | grep -E "\"(up|pending|autostart|disabled)\"|errors" -A1
      for i in $(iw dev | awk "/Interface/ { print \$2 }"); do echo "-- $i"; iw dev "$i" station dump | grep -E "^Station|plink|authorized"; done
      batctl meshif bat0 n 2>&1; ip -4 -br addr; logread | tail -n 40' 2>&1 || true
  done
}

down_containers() {
  for role in "${ROLES[@]}"; do docker rm -f "uniwrt-wifi-$role" >/dev/null 2>&1 || true; done
}

down() {
  down_containers
  rm -f "$ENV_FILE"
  if [[ -e $LOADED ]]; then
    # Radios return to the host's namespace once their container is gone.
    host_root "modprobe -r mac80211_hwsim batman-adv"
    rm -f "$LOADED"
  fi
}

# The kernel each release ships, for sanitizing: captures must not carry the host's.
declare -A KERNEL=([25.12.6]=6.12.58 [24.10.8]=6.6.110)

# Installs the working tree's agent, as version "capture".
install_agent() {
  docker exec "$1" sh -c 'rm -rf /tmp/uniwrt/agent && mkdir -p /tmp/uniwrt/agent'
  docker cp -q node-agent/. "$1:/tmp/uniwrt/agent/"
  docker exec "$1" sh -c 'echo capture >/tmp/uniwrt/agent/VERSION'
}

# Runs the working tree's agent on each node and saves its answer to collecting every set,
# sanitized like the container captures (TESTING.md): the host's model, CPU and kernel, and the
# client's hostname, which is its container ID.
capture() {
  local role release container
  for role in a b; do
    release=${IMAGE[$role]}
    container=uniwrt-wifi-$role
    install_agent "$container"
    echo '{"id":1,"op":"collect","args":{}}' |
      docker exec -i "$container" ucode /tmp/uniwrt/agent/main.uc --sync | tail -n 1 |
      python3 -c '
import json, sys
names = list(zip(sys.argv[1::2], sys.argv[2::2]))
def clean(v):
    if isinstance(v, dict): return {k: clean(x) for k, x in v.items()}
    if isinstance(v, list): return [clean(x) for x in v]
    if isinstance(v, str):
        for old, new in names: v = v.replace(old, new)
    return v
# Indented to a depth of 5 and compact below, so channel lists stay short.
def dump(v, depth=0):
    if depth >= 5 or not isinstance(v, (dict, list)) or not v:
        return json.dumps(v, ensure_ascii=False)
    pad = "  " * (depth + 1)
    if isinstance(v, dict):
        items = [pad + json.dumps(k) + ": " + dump(x, depth + 1) for k, x in v.items()]
        return "{\n" + ",\n".join(items) + "\n" + "  " * depth + "}"
    return "[\n" + ",\n".join(pad + dump(x, depth + 1) for x in v) + "\n" + "  " * depth + "]"
print(dump(clean(json.loads(sys.stdin.read())["ok"])))' "$(uname -r)" "${KERNEL[$release]}" \
        "$(docker exec "$container" ubus call system board | sed -n 's/.*"model": "\(.*\)",/\1/p')" Container \
        "$(docker exec "$container" ubus call system board | sed -n 's/.*"system": "\(.*\)",/\1/p')" \
        'Intel(R) Core(TM) CPU' \
        "$(docker inspect -f '{{.Config.Hostname}}' uniwrt-wifi-c)" lab-client \
        >"crates/node/fixtures/collect/hwsim-$role-$release.json"
  done
  # The neighbors set without rtnetlink, as on images without Wi-Fi, right after the set with it,
  # so a test can check that both describe the same caches.
  local query='{"id":1,"op":"collect","args":{"sets":["neighbors"]}}' rtnl proc
  rtnl=$(echo "$query" | docker exec -i uniwrt-wifi-a ucode /tmp/uniwrt/agent/main.uc --sync | tail -n 1)
  docker exec uniwrt-wifi-a mv /usr/lib/ucode/rtnl.so /usr/lib/ucode/rtnl.so.hidden
  proc=$(echo "$query" | docker exec -i uniwrt-wifi-a ucode /tmp/uniwrt/agent/main.uc --sync | tail -n 1)
  docker exec uniwrt-wifi-a mv /usr/lib/ucode/rtnl.so.hidden /usr/lib/ucode/rtnl.so
  printf '{"rtnl":%s,"proc":%s}' "$rtnl" "$proc" | python3 -c '
import json, sys
both = json.load(sys.stdin)
print(json.dumps({k: v["ok"] for k, v in both.items()}, indent=1))' \
    >"crates/node/fixtures/collect/hwsim-a-${IMAGE[a]}-neighbors.json"
}

# What a's agent pushes, line by line, sanitized like the collect captures: the topics' events
# while the client is kicked off a's 2g AP and comes back, loopback goes down and up, and radio0
# restarts; the log polled, then streamed; and a tick.
capture_events() {
  local release=${IMAGE[a]} kick='{"addr":"02:00:00:00:04:00","reason":5,"deauth":true,"ban_time":0}'
  # Commands run on the node print nothing: the block's output is the agent's input.
  a() { docker exec uniwrt-wifi-a "$@" >/dev/null; }
  install_agent uniwrt-wifi-a
  {
    echo '{"id":1,"op":"subscribe","args":{"topics":["dhcp","hostapd","netifd"]}}'
    echo '{"id":2,"op":"log_follow","args":{"mode":"poll","backfill":2}}'
    echo '{"id":3,"op":"set_interval","args":{"interval_ms":1000,"sets":["leases"]}}'
    sleep 1.5
    echo '{"id":4,"op":"set_interval","args":{"interval_ms":0,"sets":["leases"]}}'
    a ubus call hostapd.phy1-ap0 del_client "$kick"
    sleep 6
    echo '{"id":5,"op":"log_follow","args":{"mode":"stream"}}'
    sleep 0.5
    a sh -c 'ifdown loopback && ifup loopback && logger -t lab "loopback restarted"'
    sleep 1.5
    echo '{"id":6,"op":"log_follow","args":{"mode":"off"}}'
    a wifi down radio0
    sleep 2
    a wifi up radio0
    sleep 12
  } | docker exec -i uniwrt-wifi-a ucode /tmp/uniwrt/agent/main.uc |
    sed "s/$(docker inspect -f '{{.Config.Hostname}}' uniwrt-wifi-c)/lab-client/g" \
      >"crates/node/fixtures/events/hwsim-a-$release.jsonl"
}

case ${1:-} in
up) up ;;
capture)
  case ${2:-} in
  collect) capture ;;
  events) capture_events ;;
  '') capture && capture_events ;;
  *) echo "usage: $0 capture [collect | events]" >&2 && exit 2 ;;
  esac
  ;;
down) down ;;
test)
  shift
  [[ -e $ENV_FILE ]] || { trap down EXIT && up; }
  # shellcheck source=/dev/null # written by `up`
  set -a && source "$ENV_FILE" && set +a
  # One at a time: a test that kicks the client would fail the others' checks that it is there.
  cargo nextest run --locked -p uniwrt-node --test wifi --run-ignored only --status-level fail \
    --hide-progress-bar --test-threads 1 "$@"
  ;;
*)
  echo "usage: $0 up | down | test [nextest args…] | capture [collect | events]" >&2
  exit 2
  ;;
esac
