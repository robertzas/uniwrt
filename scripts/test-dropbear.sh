#!/usr/bin/env bash
# Runs the transport tests against dropbear on real OpenWrt (TESTING.md, Transport): for each
# OpenWrt rootfs image, starts a container with a random root password and runs
# crates/node/tests/dropbear.rs against it. Usage: scripts/test-dropbear.sh [image…]
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

for image in "${images[@]}"; do
  echo "== $image"
  password="uniwrt-$(od -An -N8 -tx8 /dev/urandom | tr -d ' ')"
  # The host key is made before dropbear starts: with -R alone, parallel first connections
  # could each generate a different one.
  container=$(docker run -d --rm -p 127.0.0.1::22 "$image" /bin/sh -c '
    printf "%s\n%s\n" "$0" "$0" | passwd root >/dev/null 2>&1 &&
    dropbearkey -t ed25519 -f /etc/dropbear/dropbear_ed25519_host_key >/dev/null 2>&1 &&
    exec /usr/sbin/dropbear -F -E -R -p 22' "$password")
  for _ in $(seq 100); do
    docker logs "$container" 2>&1 | grep -q 'Not backgrounding' && break
    sleep 0.1
  done
  port=$(docker port "$container" 22/tcp | head -n1 | sed 's/.*://')
  # dropbear drops a client's sixth concurrent unauthenticated connection (MAX_UNAUTH_PER_IP),
  # and every test connects from the Docker gateway's address.
  UNIWRT_DROPBEAR="127.0.0.1:$port" UNIWRT_DROPBEAR_PASSWORD="$password" \
    cargo nextest run --locked -p uniwrt-node --test dropbear --run-ignored only --test-threads 2 \
    --status-level fail --hide-progress-bar
  docker rm -f "$container" >/dev/null
  container=''
done
