# An OpenWrt rootfs for the system harness (scripts/test-system.sh): the stock image plus
# `bridge`, to check the bridge's VLANs, and a PPPoE server and client, for the ISP's end.
ARG IMAGE
FROM ${IMAGE}
ARG PACKAGES="ip-bridge rp-pppoe-server ppp-mod-pppoe"
# /tmp/resolv.conf is a mount while building. procd jails dnsmasq with ujail, which can't make
# namespaces in a container; without it, procd runs services unjailed.
RUN mkdir -p /var/lock /var/run && \
    if [ -x /usr/bin/apk ]; then \
      apk update && apk add $PACKAGES; \
    else \
      opkg update && opkg install $PACKAGES; \
    fi && \
    for f in /usr/sbin/bridge /usr/sbin/pppoe-server /usr/sbin/pppd; do \
      [ -e "$f" ] || { echo "missing $f" >&2; exit 1; }; \
    done && \
    { rm -rf /tmp/* 2>/dev/null || true; } && \
    rm /sbin/ujail
