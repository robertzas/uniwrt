#!/usr/bin/env bash
# The system harness (TESTING.md, System): rendered configs on real OpenWrt. For each release,
# three containers booted with procd and linked by veth pairs, with no other network
# (`--network none`, so nothing reaches the host's):
#
#   isp ── eth0 ═ eth1 (WAN) ── gw ── eth0 (LAN) ═ eth0 ── ap
#
# The ISP (scripts/system/) serves DHCP untagged (192.0.2.0/24) and on VLAN 201
# (198.51.100.0/24), and PPPoE (user@isp, 203.0.113.0/24). crates/system/tests/system.rs
# collects the gateway's and the AP's facts with the working tree's agent, renders sites for
# them, writes the packages and checks that uci, fw4 and netifd take them, through
# `docker exec`; crates/system/tests/apply/ drives the controller's apply job onto both (the
# transaction tool, the agent and the health checks; a confirm and two rollbacks).
#
# Linking the containers and loading the kernel's 8021q and pppoe modules need root, which a
# privileged helper container provides, as in the hwsim lab (D-035).
#
# Usage: scripts/test-system.sh [release…]          runs the tests on each release (default: both)
#        scripts/test-system.sh up <release> | down  starts the topology, or removes it
set -euo pipefail
cd "$(dirname "$0")/.."

# The releases in the test matrix (TESTING.md); pinned, so a run is reproducible.
RELEASES=(24.10.8 25.12.6)
ROLES=(isp gw ap)
HELPER_IMAGE=openwrt/rootfs:x86-64-25.12.6
ENV_FILE=target/system.env

host_root() {
  docker run --rm --name uniwrt-system-helper --privileged --net host --pid host -v /:/host \
    "$HELPER_IMAGE" chroot /host /bin/sh -c "$@"
}

# What the host's kernel can't do, which the tests that need it skip (crates/system/src/lib.rs):
# a host whose running kernel lost its module tree to an upgrade can't load modules. In CI the
# run fails instead.
LACKS=()
modules() {
  local module
  LACKS=()
  for module in 8021q pppoe; do
    [[ -d /sys/module/$module ]] || host_root "modprobe $module" 2>/dev/null || LACKS+=("$module")
  done
}

# fw4 loads its rules through nftables' expression modules (ct, reject, masquerade): checking
# the stock config tells whether the kernel has them.
nftables() {
  docker exec uniwrt-system-gw fw4 check >/dev/null 2>&1 || LACKS+=(nftables)
  if ((${#LACKS[@]})); then
    [[ -z ${CI:-} ]] || { echo "the host's kernel lacks ${LACKS[*]}" >&2 && exit 1; }
    echo "warning: the host's kernel lacks ${LACKS[*]} (reboot into the installed kernel?): their tests skip" >&2
  fi
  printf 'UNIWRT_SYSTEM_LACKS="%s"\n' "${LACKS[*]}" >"$ENV_FILE"
}

image() {
  docker build -q -f scripts/system-node.Dockerfile --build-arg "IMAGE=openwrt/rootfs:x86-64-$1" \
    -t "uniwrt-system-node:$1" scripts >/dev/null
}

# Runs as PID 1 before procd: waits for the role's ($0) links, and on the ISP leaves a
# uci-defaults script that applies its config once the first boot has generated the stock one.
# shellcheck disable=SC2016 # expanded by the container's shell
SETUP='
  links=eth0; [ "$0" = gw ] && links="eth0 eth1"
  for link in $links; do while [ ! -e /sys/class/net/$link ]; do sleep 1; done; done
  if [ "$0" = isp ]; then
    cp /lab/pap-secrets /etc/ppp/pap-secrets
    printf "%s\n" "uci batch </lab/isp.uci" "uci commit" >/etc/uci-defaults/99-uniwrt-isp
  fi
  exec /sbin/init'

# Each role is ready when its first interface is up (and the ISP runs dnsmasq).
declare -A READY=(
  [isp]='ubus -S call network.interface.isp status | grep -q "\"up\":true" && pidof dnsmasq'
  [gw]='ubus -S call network.interface.lan status | grep -q "\"up\":true"'
  [ap]='ubus -S call network.interface.lan status | grep -q "\"up\":true"'
)

down() {
  for role in "${ROLES[@]}"; do docker rm -f "uniwrt-system-$role" >/dev/null 2>&1 || true; done
  rm -f "$ENV_FILE"
}

up() {
  local release=$1 role
  mkdir -p target # a fresh checkout has none
  modules
  image "$release"
  down
  declare -A pid
  local ppp=()
  [[ -c /dev/ppp ]] && ppp=(--device /dev/ppp)
  for role in "${ROLES[@]}"; do
    # The sysctls, which a container can't write: the gateway routes, and every node keeps IPv6.
    docker run -d --name "uniwrt-system-$role" --hostname "$role" --network none \
      --cap-add NET_ADMIN "${ppp[@]}" \
      --sysctl net.ipv4.ip_forward=1 --sysctl net.ipv6.conf.all.forwarding=1 \
      --sysctl net.ipv6.conf.all.disable_ipv6=0 --sysctl net.ipv6.conf.default.disable_ipv6=0 \
      -v "$PWD/scripts/system:/lab:ro" "uniwrt-system-node:$release" \
      /bin/sh -c "$SETUP" "$role" >/dev/null
    pid[$role]=$(docker inspect -f '{{.State.Pid}}' "uniwrt-system-$role")
  done
  host_root "ip link add eth0 netns ${pid[isp]} type veth peer name eth1 netns ${pid[gw]} &&
    ip link add eth0 netns ${pid[gw]} type veth peer name eth0 netns ${pid[ap]}"
  for role in "${ROLES[@]}"; do
    local up=''
    for _ in $(seq 60); do
      if docker exec "uniwrt-system-$role" /bin/sh -c "${READY[$role]}" >/dev/null 2>&1; then
        up=1 && break
      fi
      sleep 1
    done
    if [[ -z $up ]]; then
      echo "$role did not come up on $release" >&2
      docker exec "uniwrt-system-$role" /bin/sh -c 'ip -br addr; logread | tail -n 40' >&2 || true
      exit 1
    fi
  done
  # The ISP neither filters nor routes IPv6, and serves PPPoE once its interface is up (procd
  # had read the boot's list of services before uci-defaults ran).
  docker exec uniwrt-system-isp /bin/sh -c '/etc/init.d/firewall stop; /etc/init.d/odhcpd stop
    /etc/init.d/pppoe-server start' >/dev/null 2>&1
  for role in gw ap; do
    docker exec "uniwrt-system-$role" sh -c 'rm -rf /tmp/uniwrt/agent && mkdir -p /tmp/uniwrt/agent'
    docker cp -q node-agent/. "uniwrt-system-$role:/tmp/uniwrt/agent/"
    docker exec "uniwrt-system-$role" sh -c 'echo system >/tmp/uniwrt/agent/VERSION'
  done
  nftables
  echo "the topology is up on $release ($ENV_FILE)"
}

case ${1:-} in
up)
  [[ -n ${2:-} ]] || { echo "usage: $0 up <release>" >&2 && exit 2; }
  up "$2"
  ;;
down) down ;;
*)
  releases=("$@")
  ((${#releases[@]})) || releases=("${RELEASES[@]}")
  trap down EXIT
  for release in "${releases[@]}"; do
    echo "== $release"
    up "$release"
    # One at a time: the tests share the topology, and each renders its own site onto it.
    # shellcheck source=/dev/null # written by `up`
    set -a && source "$ENV_FILE" && set +a
    UNIWRT_SYSTEM_RELEASE=$release cargo nextest run --locked -p uniwrt-system --test system --test apply \
      --run-ignored only --test-threads 1 --status-level fail --hide-progress-bar
    down
  done
  ;;
esac
