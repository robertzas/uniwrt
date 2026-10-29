#!/usr/bin/env bash
# Runs the agent tests against OpenWrt booted with procd (TESTING.md, Agent): for each OpenWrt
# rootfs image, boots a container with a random root password and runs crates/node/tests/agent.rs
# and crates/engine/tests/agent.rs against it. Usage: scripts/test-agent.sh [image…]
set -euo pipefail
cd "$(dirname "$0")/.."

# The releases in the test matrix (TESTING.md); pinned, so a run is reproducible.
images=("$@")
if ((${#images[@]} == 0)); then
  images=(openwrt/rootfs:x86-64-24.10.8 openwrt/rootfs:x86-64-25.12.6)
fi

container=''
cleanup() { if [[ -n $container ]]; then docker rm -f "$container" >/dev/null 2>&1 || true; fi; }
trap cleanup EXIT

# Runs as PID 1 before procd takes over: sets the root password ($0), and leaves a uci-defaults
# script that puts the container's address on lan once the first boot has generated the
# network config (netifd would otherwise move the container to 192.168.1.1). NET_ADMIN lets
# netifd configure the container's network, so `network.interface` reports real state.
# shellcheck disable=SC2016 # expanded by the container's shell
setup='
  printf "%s\n%s\n" "$0" "$0" | passwd root >/dev/null 2>&1
  address=$(ip -4 -o addr show dev eth0 | awk "{ print \$4 }")
  printf "uci set network.lan.ipaddr=%s\nuci -q delete network.lan.netmask\nuci commit network\n" \
    "$address" >/etc/uci-defaults/99-uniwrt-test
  exec /sbin/init'
# Ready: lan is up on that address and dropbear runs.
# shellcheck disable=SC2016
ready='ubus -S call network.interface.lan status 2>/dev/null | grep -q "\"up\":true" && pidof dropbear'

for image in "${images[@]}"; do
  echo "== $image"
  password="uniwrt-$(od -An -N8 -tx8 /dev/urandom | tr -d ' ')"
  container=$(docker run -d --rm --cap-add NET_ADMIN -p 127.0.0.1::22 "$image" \
    /bin/sh -c "$setup" "$password")
  port=$(docker port "$container" 22/tcp | head -n1 | sed 's/.*://')
  # Ready inside, and dropbear's banner arrives through the port mapping.
  banner() { timeout 1 bash -c "exec 3<>/dev/tcp/127.0.0.1/$port && head -c 4 <&3" 2>/dev/null; }
  up=''
  for _ in $(seq 150); do
    if docker exec "$container" /bin/sh -c "$ready" >/dev/null 2>&1 && [[ $(banner) == SSH- ]]; then
      up=1 && break
    fi
    sleep 0.2
  done
  if [[ -z $up ]]; then
    echo "OpenWrt did not come up in $image" >&2
    docker logs "$container" >&2
    exit 1
  fi
  # dropbear drops a client's sixth concurrent unauthenticated connection (MAX_UNAUTH_PER_IP).
  UNIWRT_NODE="127.0.0.1:$port" UNIWRT_NODE_PASSWORD="$password" UNIWRT_NODE_RELEASE="${image##*-}" \
    cargo nextest run --locked -p uniwrt-node -p uniwrt-engine --test agent --run-ignored only --test-threads 2 \
    --status-level fail --hide-progress-bar
  docker rm -f "$container" >/dev/null
  container=''
done
