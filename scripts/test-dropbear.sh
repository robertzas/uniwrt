#!/usr/bin/env bash
# Runs the transport tests against dropbear on real OpenWrt (TESTING.md, Transport): for each
# OpenWrt rootfs image, starts a container with a random root password and one without (factory
# defaults) and runs crates/node/tests/dropbear.rs against them. Usage: scripts/test-dropbear.sh [image…]
set -euo pipefail
cd "$(dirname "$0")/.."

# The releases in the test matrix (TESTING.md); pinned, so a run is reproducible.
images=("$@")
if ((${#images[@]} == 0)); then
  images=(openwrt/rootfs:x86-64-24.10.8 openwrt/rootfs:x86-64-25.12.6)
fi

container=''
factory=''
cleanup() {
  local name
  for name in "$container" "$factory"; do
    if [[ -n $name ]]; then docker rm -f "$name" >/dev/null 2>&1 || true; fi
  done
}
trap cleanup EXIT

# Starts dropbear in a container of image $1 and prints its id; runs `$2` first (as the shell's
# script, with the password in $0). The host key is made before dropbear starts: with -R alone,
# parallel first connections could each generate a different one. The network is named: a
# docker wrapper may default `run` to the host's network, where -p does nothing.
start() {
  docker run -d --rm --network bridge -p 127.0.0.1::22 "$1" /bin/sh -c "$2 &&
    dropbearkey -t ed25519 -f /etc/dropbear/dropbear_ed25519_host_key >/dev/null 2>&1 &&
    exec /usr/sbin/dropbear -F -E -R -p 22" "${3:-}"
}
# Waits for dropbear in container $1, then prints the host port its port 22 maps to.
port_of() {
  for _ in $(seq 100); do
    docker logs "$1" 2>&1 | grep -q 'Not backgrounding' && break
    sleep 0.1
  done
  docker port "$1" 22/tcp | head -n1 | sed 's/.*://'
}

for image in "${images[@]}"; do
  echo "== $image"
  password="uniwrt-$(od -An -N8 -tx8 /dev/urandom | tr -d ' ')"
  # shellcheck disable=SC2016 # expanded by the container's shell
  container=$(start "$image" 'printf "%s\n%s\n" "$0" "$0" | passwd root >/dev/null 2>&1' "$password")
  # Factory defaults: root has no password.
  factory=$(start "$image" true)
  port=$(port_of "$container")
  factory_port=$(port_of "$factory")
  # dropbear drops a client's sixth concurrent unauthenticated connection (MAX_UNAUTH_PER_IP),
  # and every test connects from the Docker gateway's address.
  UNIWRT_NODE="127.0.0.1:$port" UNIWRT_NODE_PASSWORD="$password" \
    UNIWRT_FACTORY_NODE="127.0.0.1:$factory_port" \
    cargo nextest run --locked -p uniwrt-node --test dropbear --run-ignored only --test-threads 2 \
    --status-level fail --hide-progress-bar
  docker rm -f "$container" "$factory" >/dev/null
  container='' factory=''
done
